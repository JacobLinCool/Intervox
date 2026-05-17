//! CPAL microphone capture with a dedicated owning thread.
//!
//! # `cpal::Stream` is `!Send` on macOS (CoreAudio)
//!
//! The stream is created, played, and dropped **entirely inside a dedicated
//! `std::thread`**.  It never leaves that thread.  `CaptureHandle` is the
//! only thing returned to the caller and is `Send` (it contains only an
//! `AtomicBool` stop flag and a `JoinHandle`).
//!
//! # Data-callback contract
//!
//! The callback is `FnMut + Send + 'static`.  It owns:
//! - A `LinearResampler` (streaming, carries phase across chunks).
//! - The `sink` sender (`SyncSender<Vec<f32>>`).
//! - The `level` `Arc<AtomicU32>` (shared with the Engine's 20 Hz emitter).
//!
//! The callback NEVER blocks and never allocates after stream construction:
//! it borrows preallocated buffers from a bounded pool, fills one, and
//! `try_send`s the frame. Full/disconnected channels drop the frame and return
//! the buffer to the pool.

use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::FromSample;
use cpal::Sample;
use cpal::SizedSample;
use intervox_core::audio::level_meter::LevelMeter;
use intervox_core::audio::resampler::LinearResampler;
use intervox_core::AppError;
use serde::Serialize;
use tauri::Emitter;

use super::AudioBackpressureCounters;

/// Target sample rate for the engine (virtual mic + OpenAI path).
const TARGET_HZ: u32 = 48_000;

/// Capacity of the bounded inter-thread channel.
const SINK_BOUND: usize = 64;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CALLBACK_FRAMES: usize = 16_384;
const MAX_CAPTURE_OUTPUT_FRAMES: usize = 32_768;

// ── CaptureHandle ─────────────────────────────────────────────────────────────

/// `Send` handle to the dedicated capture thread.
///
/// Dropping this handle signals the thread to stop and blocks until the thread
/// exits (joining cleanly drops the `cpal::Stream` inside the thread).
pub struct CaptureHandle {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

pub struct CapturedFrame {
    samples: Vec<f32>,
    pool: SyncSender<Vec<f32>>,
}

impl CapturedFrame {
    fn new(samples: Vec<f32>, pool: SyncSender<Vec<f32>>) -> Self {
        Self { samples, pool }
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.samples
    }
}

impl Deref for CapturedFrame {
    type Target = [f32];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl Drop for CapturedFrame {
    fn drop(&mut self) {
        let mut samples = std::mem::take(&mut self.samples);
        samples.clear();
        let _ = self.pool.try_send(samples);
    }
}

impl CaptureHandle {
    /// Signal the capture thread to stop and wait for it to exit.
    #[allow(dead_code)]
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ── Pure helpers ──────────────────────────────────────────────────────────────

/// Downmix interleaved N-channel audio to mono by averaging channels per frame.
///
/// `interleaved`: samples in [L, R, ...] order.
/// `channels`: number of channels (≥ 1).
///
/// Returns a `Vec<f32>` of length `interleaved.len() / channels as usize`.
/// Panics if `channels == 0`.
pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    assert!(channels > 0, "channels must be >= 1");
    let mut out = Vec::with_capacity(interleaved.len() / channels as usize);
    downmix_to_mono_f32_into(interleaved, channels, &mut out);
    out
}

fn downmix_to_mono_f32_into(interleaved: &[f32], channels: u16, out: &mut Vec<f32>) {
    assert!(channels > 0, "channels must be >= 1");
    let ch = channels as usize;
    out.clear();
    if interleaved.len() / ch > out.capacity() {
        return;
    }
    for frame in interleaved.chunks_exact(ch) {
        out.push(frame.iter().copied().sum::<f32>() / ch as f32);
    }
}

fn downmix_converted_to_mono_into<T>(interleaved: &[T], channels: u16, out: &mut Vec<f32>)
where
    T: SizedSample + Copy,
    f32: FromSample<T>,
{
    assert!(channels > 0, "channels must be >= 1");
    let ch = channels as usize;
    out.clear();
    if interleaved.len() / ch > out.capacity() {
        return;
    }
    for frame in interleaved.chunks_exact(ch) {
        let mut sum = 0.0f32;
        for &sample in frame {
            sum += f32::from_sample(sample);
        }
        out.push(sum / ch as f32);
    }
}

fn resampler_has_capacity(
    resampler: &LinearResampler,
    input_len: usize,
    output: &Vec<f32>,
) -> bool {
    input_len <= MAX_CALLBACK_FRAMES && resampler.max_output_len(input_len) <= output.capacity()
}

// ── Device resolution ─────────────────────────────────────────────────────────

/// Resolve a cpal input device from a frontend `device_id`.
///
/// Frontend IDs have the form `"coreaudio:<name>"`.  Strip the prefix and
/// search by `device.name()`. Use `host.devices()` instead of
/// `host.input_devices()` so resolving a selected mic does not ask every
/// device for supported stream configs before opening the one stream we need.
/// Uses the system default only when no explicit `device_id` is provided.
/// An explicit but missing device is a hard error so the UI cannot claim it is
/// listening to one microphone while CPAL silently captures another.
fn resolve_input_device(device_id: Option<&str>) -> Result<cpal::Device, AppError> {
    let host = cpal::default_host();

    if let Some(id) = device_id {
        let target_name = id.strip_prefix("coreaudio:").unwrap_or(id);
        let devices = host
            .devices()
            .map_err(|e| AppError::internal(format!("enumerate CoreAudio devices: {e}")))?;
        let mut matched_non_input = false;

        for dev in devices {
            if let Ok(name) = dev.name() {
                if name == target_name {
                    if dev.default_input_config().is_ok() {
                        return Ok(dev);
                    }
                    matched_non_input = true;
                }
            }
        }

        if matched_non_input {
            return Err(AppError::audio_device_unavailable(format!(
                "The selected device '{target_name}' exists but does not expose an input stream."
            )));
        }

        return Err(AppError::audio_device_unavailable(format!(
            "The selected microphone '{target_name}' is not visible to CoreAudio."
        )));
    }

    host.default_input_device()
        .ok_or_else(AppError::audio_device_lost)
}

fn resolved_device_name(device: &cpal::Device) -> String {
    device.name().unwrap_or_else(|_| "<unknown>".into())
}

// ── Stream builder (generic over sample format) ───────────────────────────────

/// Build a cpal input stream for a device whose sample format is `T`.
///
/// The callback:
/// 1. Converts samples to `f32` (via `f32::from_sample`).
/// 2. Downmixes interleaved N-ch → mono.
/// 3. Resamples to `TARGET_HZ` with the stateful `LinearResampler`.
/// 4. Computes level and stores `rms` bits into `level`.
/// 5. `try_send`s the frame — drops on full/disconnected (never blocks).
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sink: SyncSender<CapturedFrame>,
    pool_tx: SyncSender<Vec<f32>>,
    pool_rx: Receiver<Vec<f32>>,
    level: Arc<AtomicU32>,
    stream_error: Arc<AtomicBool>,
    backpressure: Arc<AudioBackpressureCounters>,
) -> Result<cpal::Stream, AppError>
where
    T: SizedSample + Send + Copy + 'static,
    f32: FromSample<T>,
{
    let channels = config.channels;
    let in_hz = config.sample_rate.0;
    let mut resampler = LinearResampler::new(in_hz, TARGET_HZ);
    resampler.reserve_for(MAX_CALLBACK_FRAMES);
    let mut mono = Vec::with_capacity(MAX_CALLBACK_FRAMES);

    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let input_frames = data.len() / usize::from(channels);
                if input_frames == 0 {
                    return;
                }
                if input_frames > MAX_CALLBACK_FRAMES {
                    backpressure.capture_capacity_drop();
                    return;
                }

                downmix_converted_to_mono_into(data, channels, &mut mono);
                if mono.is_empty() || mono.len() > MAX_CALLBACK_FRAMES {
                    backpressure.capture_capacity_drop();
                    return;
                }

                let Ok(mut resampled) = pool_rx.try_recv() else {
                    backpressure.capture_pool_miss();
                    return;
                };
                if !resampler_has_capacity(&resampler, mono.len(), &resampled) {
                    let _ = pool_tx.try_send(resampled);
                    backpressure.capture_capacity_drop();
                    return;
                }
                resampler.process_into(&mono, &mut resampled);

                let audio_level = LevelMeter::measure(&resampled);
                level.store(audio_level.rms.to_bits(), Ordering::Relaxed);

                if sink
                    .try_send(CapturedFrame::new(resampled, pool_tx.clone()))
                    .is_err()
                {
                    backpressure.capture_sink_drop();
                    // The frame is dropped here; CapturedFrame::drop returns
                    // the buffer to the pool immediately.
                }
            },
            move |err| {
                let _ = err;
                stream_error.store(true, Ordering::Release);
            },
            None,
        )
        .map_err(|e| AppError::internal(format!("cpal build_input_stream: {e}")))?;

    Ok(stream)
}

fn update_max_rms(max_rms: &AtomicU32, value: f32) {
    let value = value.clamp(0.0, 1.0);
    let mut current = max_rms.load(Ordering::Relaxed);
    loop {
        let current_value = f32::from_bits(current);
        if value <= current_value {
            return;
        }
        match max_rms.compare_exchange_weak(
            current,
            value.to_bits(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(next) => current = next,
        }
    }
}

fn build_probe_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    max_rms: Arc<AtomicU32>,
    callback_count: Arc<AtomicU64>,
    captured_frames: Arc<AtomicU64>,
    last_error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, AppError>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let channels = config.channels;
    let in_hz = config.sample_rate.0;
    let mut resampler = LinearResampler::new(in_hz, TARGET_HZ);
    resampler.reserve_for(MAX_CALLBACK_FRAMES);
    let mut mono = Vec::with_capacity(MAX_CALLBACK_FRAMES);
    let mut resampled = Vec::with_capacity(MAX_CAPTURE_OUTPUT_FRAMES);

    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                callback_count.fetch_add(1, Ordering::Relaxed);

                downmix_converted_to_mono_into(data, channels, &mut mono);
                if mono.is_empty() || !resampler_has_capacity(&resampler, mono.len(), &resampled) {
                    return;
                }
                captured_frames.fetch_add(mono.len() as u64, Ordering::Relaxed);

                resampler.process_into(&mono, &mut resampled);
                let level = LevelMeter::measure(&resampled);
                update_max_rms(&max_rms, level.rms);
            },
            move |err| {
                *last_error.lock().unwrap() = Some(err.to_string());
            },
            None,
        )
        .map_err(|e| AppError::internal(format!("cpal build_input_stream: {e}")))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureProbeReport {
    pub requested_device_id: Option<String>,
    pub resolved_device_name: String,
    pub sample_format: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub callback_count: u64,
    pub captured_frames: u64,
    pub max_rms: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_error: Option<String>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Start microphone capture on a dedicated owning thread.
///
/// - `device_id`: optional frontend device id (`"coreaudio:<name>"`).
/// - `level`: shared `AtomicU32` written with `rms.to_bits()` on every chunk.
/// - `app`: used to emit `"error"` events from the cpal error callback.
///
/// Returns a `(CaptureHandle, Receiver<Vec<f32>>)`.  The caller owns the
/// receiver; the capture thread owns the sender side.
pub fn start(
    device_id: Option<&str>,
    level: Arc<AtomicU32>,
    app: tauri::AppHandle,
    backpressure: Arc<AudioBackpressureCounters>,
) -> Result<(CaptureHandle, std::sync::mpsc::Receiver<CapturedFrame>), AppError> {
    let device = resolve_input_device(device_id)?;

    let supported_config = device
        .default_input_config()
        .map_err(|e| AppError::internal(format!("default_input_config: {e}")))?;

    let stream_config: cpal::StreamConfig = supported_config.config();
    let sample_format = supported_config.sample_format();

    let (tx, rx) = std::sync::mpsc::sync_channel::<CapturedFrame>(SINK_BOUND);
    let (pool_tx, pool_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(SINK_BOUND);
    for _ in 0..SINK_BOUND {
        pool_tx
            .try_send(Vec::with_capacity(MAX_CAPTURE_OUTPUT_FRAMES))
            .map_err(|_| AppError::internal("capture buffer pool initialization failed"))?;
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), AppError>>(1);
    let stream_error = Arc::new(AtomicBool::new(false));

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    let thread = std::thread::Builder::new()
        .name("capture".to_string())
        .spawn(move || {
            let mut tx_slot = Some(tx);
            let mut pool_tx_slot = Some(pool_tx);
            let mut pool_rx_slot = Some(pool_rx);
            let mut level_slot = Some(level);
            let mut backpressure_slot = Some(backpressure);

            macro_rules! build_for_format {
                ($sample:ty) => {
                    build_stream::<$sample>(
                        &device,
                        &stream_config,
                        tx_slot.take().expect("capture sender taken once"),
                        pool_tx_slot.take().expect("capture pool sender taken once"),
                        pool_rx_slot.take().expect("capture pool receiver taken once"),
                        level_slot.take().expect("capture level taken once"),
                        Arc::clone(&stream_error),
                        backpressure_slot
                            .take()
                            .expect("capture backpressure counters taken once"),
                    )
                };
            }

            // Build the stream inside this thread — cpal::Stream stays here.
            let stream_result = match sample_format {
                cpal::SampleFormat::I8 => build_for_format!(i8),
                cpal::SampleFormat::F32 => build_for_format!(f32),
                cpal::SampleFormat::I16 => build_for_format!(i16),
                cpal::SampleFormat::I32 => build_for_format!(i32),
                cpal::SampleFormat::I64 => build_for_format!(i64),
                cpal::SampleFormat::U8 => build_for_format!(u8),
                cpal::SampleFormat::U16 => build_for_format!(u16),
                cpal::SampleFormat::U32 => build_for_format!(u32),
                cpal::SampleFormat::U64 => build_for_format!(u64),
                cpal::SampleFormat::F64 => build_for_format!(f64),
                other => {
                    let err =
                        AppError::internal(format!("unsupported input sample format: {other:?}"));
                    let _ = ready_tx.send(Err(err));
                    return;
                }
            };

            let stream = match stream_result {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };

            if let Err(e) = stream.play() {
                let _ = ready_tx.send(Err(AppError::internal(format!("cpal stream.play: {e}"))));
                return;
            }
            let _ = ready_tx.send(Ok(()));

            // Park this thread until the stop flag is set.
            while !stop_thread.load(Ordering::Acquire) {
                if stream_error.swap(false, Ordering::AcqRel) {
                    let _ = app.emit("error", AppError::audio_device_lost());
                }
                std::thread::park_timeout(std::time::Duration::from_millis(50));
            }

            // `stream` is dropped here → CoreAudio tears down the session.
        })
        .map_err(|e| AppError::internal(format!("spawn capture thread: {e}")))?;

    match ready_rx.recv_timeout(STARTUP_TIMEOUT) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = thread.join();
            return Err(e);
        }
        Err(RecvTimeoutError::Timeout) => {
            stop.store(true, Ordering::Release);
            thread.thread().unpark();
            return Err(AppError::internal(format!(
                "capture startup timed out after {}s",
                STARTUP_TIMEOUT.as_secs()
            )));
        }
        Err(RecvTimeoutError::Disconnected) => {
            let _ = thread.join();
            return Err(AppError::internal("capture thread exited before startup"));
        }
    }

    Ok((
        CaptureHandle {
            stop,
            thread: Some(thread),
        },
        rx,
    ))
}

/// Open the selected input device for a bounded duration and report whether
/// CoreAudio delivered real input callbacks. Used by the packaged-app CLI probe
/// and manual acceptance when the UI meter is suspected of lying.
pub fn probe_level(
    device_id: Option<&str>,
    duration: Duration,
) -> Result<CaptureProbeReport, AppError> {
    let device = resolve_input_device(device_id)?;
    let resolved_name = resolved_device_name(&device);
    let supported_config = device
        .default_input_config()
        .map_err(|e| AppError::internal(format!("default_input_config: {e}")))?;
    let stream_config: cpal::StreamConfig = supported_config.config();
    let sample_format = supported_config.sample_format();

    let max_rms = Arc::new(AtomicU32::new(0));
    let callback_count = Arc::new(AtomicU64::new(0));
    let captured_frames = Arc::new(AtomicU64::new(0));
    let last_error = Arc::new(Mutex::new(None));

    let stream_result = match sample_format {
        cpal::SampleFormat::I8 => build_probe_stream::<i8>(
            &device,
            &stream_config,
            Arc::clone(&max_rms),
            Arc::clone(&callback_count),
            Arc::clone(&captured_frames),
            Arc::clone(&last_error),
        ),
        cpal::SampleFormat::F32 => build_probe_stream::<f32>(
            &device,
            &stream_config,
            Arc::clone(&max_rms),
            Arc::clone(&callback_count),
            Arc::clone(&captured_frames),
            Arc::clone(&last_error),
        ),
        cpal::SampleFormat::I16 => build_probe_stream::<i16>(
            &device,
            &stream_config,
            Arc::clone(&max_rms),
            Arc::clone(&callback_count),
            Arc::clone(&captured_frames),
            Arc::clone(&last_error),
        ),
        cpal::SampleFormat::I32 => build_probe_stream::<i32>(
            &device,
            &stream_config,
            Arc::clone(&max_rms),
            Arc::clone(&callback_count),
            Arc::clone(&captured_frames),
            Arc::clone(&last_error),
        ),
        cpal::SampleFormat::I64 => build_probe_stream::<i64>(
            &device,
            &stream_config,
            Arc::clone(&max_rms),
            Arc::clone(&callback_count),
            Arc::clone(&captured_frames),
            Arc::clone(&last_error),
        ),
        cpal::SampleFormat::U8 => build_probe_stream::<u8>(
            &device,
            &stream_config,
            Arc::clone(&max_rms),
            Arc::clone(&callback_count),
            Arc::clone(&captured_frames),
            Arc::clone(&last_error),
        ),
        cpal::SampleFormat::U16 => build_probe_stream::<u16>(
            &device,
            &stream_config,
            Arc::clone(&max_rms),
            Arc::clone(&callback_count),
            Arc::clone(&captured_frames),
            Arc::clone(&last_error),
        ),
        cpal::SampleFormat::U32 => build_probe_stream::<u32>(
            &device,
            &stream_config,
            Arc::clone(&max_rms),
            Arc::clone(&callback_count),
            Arc::clone(&captured_frames),
            Arc::clone(&last_error),
        ),
        cpal::SampleFormat::U64 => build_probe_stream::<u64>(
            &device,
            &stream_config,
            Arc::clone(&max_rms),
            Arc::clone(&callback_count),
            Arc::clone(&captured_frames),
            Arc::clone(&last_error),
        ),
        cpal::SampleFormat::F64 => build_probe_stream::<f64>(
            &device,
            &stream_config,
            Arc::clone(&max_rms),
            Arc::clone(&callback_count),
            Arc::clone(&captured_frames),
            Arc::clone(&last_error),
        ),
        other => Err(AppError::internal(format!(
            "unsupported input sample format: {other:?}"
        ))),
    }?;

    stream_result
        .play()
        .map_err(|e| AppError::internal(format!("cpal stream.play: {e}")))?;
    std::thread::sleep(duration);
    drop(stream_result);
    let stream_error = last_error.lock().unwrap().clone();

    Ok(CaptureProbeReport {
        requested_device_id: device_id.map(str::to_owned),
        resolved_device_name: resolved_name,
        sample_format: sample_format.to_string(),
        sample_rate: stream_config.sample_rate.0,
        channels: stream_config.channels,
        callback_count: callback_count.load(Ordering::Relaxed),
        captured_frames: captured_frames.load(Ordering::Relaxed),
        max_rms: f32::from_bits(max_rms.load(Ordering::Relaxed)),
        stream_error,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── downmix_to_mono ───────────────────────────────────────────────────────

    #[test]
    fn downmix_mono_passthrough() {
        let input = vec![0.1, 0.5, -0.3, 1.0];
        let out = downmix_to_mono(&input, 1);
        assert_eq!(out, input, "mono passthrough must be identical");
        assert_eq!(out.len(), input.len());
    }

    #[test]
    fn downmix_stereo_average() {
        // L=1.0, R=0.0 → mono should be 0.5 for each frame.
        let input = vec![1.0_f32, 0.0, 1.0, 0.0];
        let out = downmix_to_mono(&input, 2);
        assert_eq!(out.len(), 2, "stereo → mono halves frame count");
        for v in &out {
            assert!((v - 0.5_f32).abs() < 1e-6, "expected 0.5, got {v}");
        }
    }

    #[test]
    fn downmix_quad_average() {
        // 4 channels: 0.0, 0.4, 0.8, 0.8 → mean = 0.5
        let input = vec![0.0_f32, 0.4, 0.8, 0.8];
        let out = downmix_to_mono(&input, 4);
        assert_eq!(out.len(), 1);
        assert!((out[0] - 0.5_f32).abs() < 1e-6, "got {}", out[0]);
    }

    #[test]
    fn downmix_length_equals_input_len_div_channels() {
        for channels in 1_u16..=6 {
            let n = channels as usize * 1000;
            let input: Vec<f32> = (0..n).map(|i| i as f32 * 0.001).collect();
            let out = downmix_to_mono(&input, channels);
            assert_eq!(
                out.len(),
                input.len() / channels as usize,
                "channels={channels}"
            );
        }
    }

    #[test]
    fn update_max_rms_keeps_largest_value() {
        let max = AtomicU32::new(0.2_f32.to_bits());
        update_max_rms(&max, 0.1);
        assert_eq!(f32::from_bits(max.load(Ordering::Relaxed)), 0.2);
        update_max_rms(&max, 0.4);
        assert_eq!(f32::from_bits(max.load(Ordering::Relaxed)), 0.4);
    }

    #[test]
    fn downmix_stereo_negative_average() {
        // L=-1.0, R=1.0 → should average to 0.0
        let input = vec![-1.0_f32, 1.0, -1.0, 1.0];
        let out = downmix_to_mono(&input, 2);
        assert_eq!(out.len(), 2);
        for v in &out {
            assert!(v.abs() < 1e-6, "expected 0.0, got {v}");
        }
    }

    // ── Hardware capture (ignored on CI) ──────────────────────────────────────

    #[test]
    #[ignore]
    fn hardware_capture_starts_and_stops() {
        let _level = Arc::new(AtomicU32::new(0));
        // NOTE: This test requires a real microphone and a valid Tauri
        // AppHandle — it cannot run in a unit-test context without a full
        // Tauri runtime.  Mark #[ignore] to prevent CI execution.
    }
}

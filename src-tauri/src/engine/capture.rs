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
//! The callback NEVER blocks: `try_send` drops the frame on full/disconnected.
//! No unbounded allocation beyond the unavoidable resampled `Vec`.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::FromSample;
use cpal::Sample;
use cpal::SizedSample;
use intervox_core::audio::level_meter::LevelMeter;
use intervox_core::audio::resampler::LinearResampler;
use intervox_core::AppError;
use tauri::Emitter;

/// Target sample rate for the engine (virtual mic + OpenAI path).
const TARGET_HZ: u32 = 48_000;

/// Capacity of the bounded inter-thread channel.
const SINK_BOUND: usize = 64;

// ── CaptureHandle ─────────────────────────────────────────────────────────────

/// `Send` handle to the dedicated capture thread.
///
/// Dropping this handle signals the thread to stop and blocks until the thread
/// exits (joining cleanly drops the `cpal::Stream` inside the thread).
pub struct CaptureHandle {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
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
    let ch = channels as usize;
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().copied().sum::<f32>() / ch as f32)
        .collect()
}

// ── Device resolution ─────────────────────────────────────────────────────────

/// Resolve a cpal input device from a frontend `device_id`.
///
/// Frontend IDs have the form `"coreaudio:<name>"`.  Strip the prefix and
/// search by `device.name()`.  Falls back to the system default on `None` or
/// no match.
fn resolve_input_device(device_id: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();

    if let Some(id) = device_id {
        let target_name = id.strip_prefix("coreaudio:").unwrap_or(id);
        if let Ok(devices) = host.input_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    if name == target_name {
                        return Some(dev);
                    }
                }
            }
        }
    }

    host.default_input_device()
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
    sink: SyncSender<Vec<f32>>,
    level: Arc<AtomicU32>,
    app: tauri::AppHandle,
) -> Result<cpal::Stream, AppError>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let channels = config.channels;
    let in_hz = config.sample_rate.0;
    let mut resampler = LinearResampler::new(in_hz, TARGET_HZ);

    let err_app = app.clone();
    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                // 1. Convert to f32.
                let f32_samples: Vec<f32> =
                    data.iter().copied().map(|s| f32::from_sample(s)).collect();

                // 2. Downmix to mono.
                let mono = downmix_to_mono(&f32_samples, channels);

                // 3. Resample to TARGET_HZ.
                let resampled = resampler.process(&mono);

                // 4. Measure level.
                let audio_level = LevelMeter::measure(&resampled);
                level.store(audio_level.rms.to_bits(), Ordering::Relaxed);

                // 5. Non-blocking push — drop on back-pressure.
                let _ = sink.try_send(resampled);
            },
            move |err| {
                let _ = err_app.emit("error", AppError::audio_device_lost());
                eprintln!("[capture] cpal stream error: {err}");
            },
            None,
        )
        .map_err(|e| AppError::internal(format!("cpal build_input_stream: {e}")))?;

    Ok(stream)
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
) -> Result<
    (
        CaptureHandle,
        std::sync::mpsc::Receiver<Vec<f32>>,
    ),
    AppError,
> {
    let device = resolve_input_device(device_id)
        .ok_or_else(AppError::audio_device_lost)?;

    let supported_config = device
        .default_input_config()
        .map_err(|e| AppError::internal(format!("default_input_config: {e}")))?;

    let stream_config: cpal::StreamConfig = supported_config.config();
    let sample_format = supported_config.sample_format();

    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(SINK_BOUND);

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    let thread = std::thread::Builder::new()
        .name("capture".to_string())
        .spawn(move || {
            // Build the stream inside this thread — cpal::Stream stays here.
            let stream_result = match sample_format {
                cpal::SampleFormat::F32 => build_stream::<f32>(
                    &device,
                    &stream_config,
                    tx,
                    level,
                    app,
                ),
                cpal::SampleFormat::I16 => build_stream::<i16>(
                    &device,
                    &stream_config,
                    tx,
                    level,
                    app,
                ),
                cpal::SampleFormat::U16 => build_stream::<u16>(
                    &device,
                    &stream_config,
                    tx,
                    level,
                    app,
                ),
                other => {
                    eprintln!("[capture] unsupported sample format: {other:?}");
                    return;
                }
            };

            let stream = match stream_result {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[capture] failed to build stream: {e}");
                    return;
                }
            };

            if let Err(e) = stream.play() {
                eprintln!("[capture] stream.play() failed: {e}");
                return;
            }

            // Park this thread until the stop flag is set.
            while !stop_thread.load(Ordering::Acquire) {
                std::thread::park_timeout(std::time::Duration::from_millis(50));
            }

            // `stream` is dropped here → CoreAudio tears down the session.
        })
        .map_err(|e| AppError::internal(format!("spawn capture thread: {e}")))?;

    Ok((CaptureHandle { stop, thread: Some(thread) }, rx))
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
            assert!(
                (v - 0.5_f32).abs() < 1e-6,
                "expected 0.5, got {v}"
            );
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

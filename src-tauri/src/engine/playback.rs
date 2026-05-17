//! Local output preview for the final 48 kHz mono virtual-mic signal.
//!
//! Preview is a best-effort mirror: it must never block capture, translation,
//! or the HAL virtual microphone. The producer side uses a bounded non-blocking
//! queue. The CPAL output stream lives entirely inside its owning thread, which
//! matches the capture-side CoreAudio ownership pattern.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use intervox_core::audio::resampler::LinearResampler;
use intervox_core::AppError;

pub type PlaybackSlot = Arc<parking_lot::Mutex<Option<PlaybackSender>>>;

const INPUT_SAMPLE_RATE: u32 = 48_000;
const QUEUE_BOUND: usize = 8;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_PREVIEW_FRAME_SAMPLES: usize = 32_768;

#[derive(Debug)]
enum PlaybackMessage {
    Audio { generation: u64, samples: Vec<f32> },
    Clear { generation: u64 },
}

#[derive(Clone)]
pub struct PlaybackSender {
    tx: SyncSender<PlaybackMessage>,
    generation: Arc<AtomicU64>,
}

impl PlaybackSender {
    pub fn try_send(&self, samples: &[f32]) -> bool {
        if samples.is_empty() || samples.len() > MAX_PREVIEW_FRAME_SAMPLES {
            return false;
        }
        let generation = self.generation.load(Ordering::Acquire);
        let message = PlaybackMessage::Audio {
            generation,
            samples: samples.to_vec(),
        };
        match self.tx.try_send(message) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub fn clear(&self) {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let _ = self.tx.try_send(PlaybackMessage::Clear { generation });
    }
}

pub struct PlaybackHandle {
    device_id: String,
    sender: PlaybackSender,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PlaybackHandle {
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn sender(&self) -> PlaybackSender {
        self.sender.clone()
    }

    fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.sender.clear();
        if let Some(thread) = self.thread.as_ref() {
            thread.thread().unpark();
        }
    }

    pub fn stop_in_background(mut self) {
        self.request_stop();
        if let Some(thread) = self.thread.take() {
            let spawn_result = std::thread::Builder::new()
                .name("output-preview-stop".to_string())
                .spawn(move || {
                    let started = std::time::Instant::now();
                    let _ = thread.join();
                    let elapsed = started.elapsed();
                    if elapsed > Duration::from_secs(2) {
                        eprintln!(
                            "[engine] output-preview stop join completed after {} ms",
                            elapsed.as_millis()
                        );
                    }
                });
            if spawn_result.is_err() {
                eprintln!("[engine] failed to spawn output-preview stop reaper");
            }
        }
    }
}

impl Drop for PlaybackHandle {
    fn drop(&mut self) {
        self.request_stop();
    }
}

pub fn tap(sender: Option<PlaybackSender>, samples: &[f32]) {
    if let Some(sender) = sender {
        let _ = sender.try_send(samples);
    }
}

pub fn tap_slot(slot: &PlaybackSlot, samples: &[f32]) {
    let sender = slot.lock().clone();
    tap(sender, samples);
}

pub fn clear_slot(slot: &PlaybackSlot) {
    let sender = slot.lock().clone();
    if let Some(sender) = sender {
        sender.clear();
    }
}

pub fn start_default_output() -> Result<PlaybackHandle, AppError> {
    let device_id = crate::devices::default_output_device_id().ok_or_else(|| {
        AppError::audio_device_unavailable("No macOS default output device is available.")
    })?;
    start_default_output_for_device_id(device_id)
}

pub fn start_default_output_for_device_id(device_id: String) -> Result<PlaybackHandle, AppError> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| {
        AppError::audio_device_unavailable("No default output device is available.")
    })?;
    let supported_config = device
        .default_output_config()
        .map_err(|e| AppError::internal(format!("default_output_config: {e}")))?;
    let stream_config: cpal::StreamConfig = supported_config.config();
    let sample_format = supported_config.sample_format();

    let (tx, rx) = std::sync::mpsc::sync_channel::<PlaybackMessage>(QUEUE_BOUND);
    let generation = Arc::new(AtomicU64::new(0));
    let sender = PlaybackSender {
        tx,
        generation: Arc::clone(&generation),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), AppError>>(1);
    let stream_error = Arc::new(AtomicBool::new(false));
    let stream_error_thread = Arc::clone(&stream_error);

    let thread = std::thread::Builder::new()
        .name("output-preview".to_string())
        .spawn(move || {
            let mut rx_slot = Some(rx);
            let mut generation_slot = Some(generation);
            let mut error_slot = Some(stream_error_thread);

            macro_rules! build_for_format {
                ($sample:ty) => {
                    build_stream::<$sample>(
                        &device,
                        &stream_config,
                        rx_slot.take().expect("playback receiver taken once"),
                        generation_slot
                            .take()
                            .expect("playback generation taken once"),
                        error_slot.take().expect("playback error flag taken once"),
                    )
                };
            }

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
                        AppError::internal(format!("unsupported output sample format: {other:?}"));
                    let _ = ready_tx.send(Err(err));
                    return;
                }
            };

            let stream = match stream_result {
                Ok(stream) => stream,
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
            };

            if let Err(e) = stream.play() {
                let _ = ready_tx.send(Err(AppError::internal(format!(
                    "cpal output stream.play: {e}"
                ))));
                return;
            }
            let _ = ready_tx.send(Ok(()));

            while !stop_thread.load(Ordering::Acquire) {
                if stream_error.swap(false, Ordering::AcqRel) {
                    eprintln!("[engine] output-preview stream error");
                }
                std::thread::park_timeout(Duration::from_millis(50));
            }
        })
        .map_err(|e| AppError::internal(format!("spawn output preview thread: {e}")))?;

    match ready_rx.recv_timeout(STARTUP_TIMEOUT) {
        Ok(Ok(())) => Ok(PlaybackHandle {
            device_id,
            sender,
            stop,
            thread: Some(thread),
        }),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(RecvTimeoutError::Timeout) => {
            stop.store(true, Ordering::Release);
            thread.thread().unpark();
            Err(AppError::internal(format!(
                "output preview startup timed out after {}s",
                STARTUP_TIMEOUT.as_secs()
            )))
        }
        Err(RecvTimeoutError::Disconnected) => {
            let _ = thread.join();
            Err(AppError::internal(
                "output preview thread exited before startup",
            ))
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    rx: Receiver<PlaybackMessage>,
    generation: Arc<AtomicU64>,
    stream_error: Arc<AtomicBool>,
) -> Result<cpal::Stream, AppError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    let out_hz = config.sample_rate.0;
    let mut state = PreviewOutputState::new(rx, generation, out_hz);

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let sample = state.next_sample().clamp(-1.0, 1.0);
                    let value = T::from_sample(sample);
                    for channel in frame {
                        *channel = value;
                    }
                }
            },
            move |err| {
                let _ = err;
                stream_error.store(true, Ordering::Release);
            },
            None,
        )
        .map_err(|e| AppError::internal(format!("cpal build_output_stream: {e}")))
}

struct PreviewOutputState {
    rx: Receiver<PlaybackMessage>,
    generation: Arc<AtomicU64>,
    active_generation: u64,
    resampler: LinearResampler,
    resampled: Vec<f32>,
    resampled_pos: usize,
}

impl PreviewOutputState {
    fn new(rx: Receiver<PlaybackMessage>, generation: Arc<AtomicU64>, out_hz: u32) -> Self {
        let active_generation = generation.load(Ordering::Acquire);
        let mut resampler = LinearResampler::new(INPUT_SAMPLE_RATE, out_hz);
        resampler.reserve_for(MAX_PREVIEW_FRAME_SAMPLES);
        let resampled_capacity = resampler.max_output_len(MAX_PREVIEW_FRAME_SAMPLES);
        Self {
            rx,
            generation,
            active_generation,
            resampler,
            resampled: Vec::with_capacity(resampled_capacity),
            resampled_pos: 0,
        }
    }

    fn next_sample(&mut self) -> f32 {
        self.sync_generation();
        loop {
            if self.resampled_pos < self.resampled.len() {
                let sample = self.resampled[self.resampled_pos];
                self.resampled_pos += 1;
                return sample;
            }

            if !self.load_next_audio_block() {
                return 0.0;
            }
        }
    }

    fn sync_generation(&mut self) {
        let latest = self.generation.load(Ordering::Acquire);
        if latest != self.active_generation {
            self.active_generation = latest;
            self.reset_audio_state();
            self.drain_stale_messages();
        }
    }

    fn reset_audio_state(&mut self) {
        self.resampler = LinearResampler::new(INPUT_SAMPLE_RATE, self.resampler.out_hz);
        self.resampler.reserve_for(MAX_PREVIEW_FRAME_SAMPLES);
        self.resampled.clear();
        self.resampled_pos = 0;
    }

    fn drain_stale_messages(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(PlaybackMessage::Audio {
                    generation,
                    samples,
                }) => {
                    if generation == self.active_generation {
                        self.resampler.process_into(&samples, &mut self.resampled);
                        self.resampled_pos = 0;
                        return;
                    }
                }
                Ok(PlaybackMessage::Clear { generation }) => {
                    self.active_generation = generation;
                    self.reset_audio_state();
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return,
            }
        }
    }

    fn load_next_audio_block(&mut self) -> bool {
        loop {
            match self.rx.try_recv() {
                Ok(PlaybackMessage::Audio {
                    generation,
                    samples,
                }) => {
                    if generation != self.active_generation {
                        continue;
                    }
                    self.resampler.process_into(&samples, &mut self.resampled);
                    self.resampled_pos = 0;
                    if !self.resampled.is_empty() {
                        return true;
                    }
                }
                Ok(PlaybackMessage::Clear { generation }) => {
                    self.generation.store(generation, Ordering::Release);
                    self.active_generation = generation;
                    self.reset_audio_state();
                    return false;
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_sender(bound: usize) -> (PlaybackSender, Receiver<PlaybackMessage>, Arc<AtomicU64>) {
        let (tx, rx) = std::sync::mpsc::sync_channel::<PlaybackMessage>(bound);
        let generation = Arc::new(AtomicU64::new(0));
        (
            PlaybackSender {
                tx,
                generation: Arc::clone(&generation),
            },
            rx,
            generation,
        )
    }

    #[test]
    fn preview_sender_drops_when_bounded_queue_is_full() {
        let (sender, _rx, _generation) = test_sender(1);

        assert!(sender.try_send(&[0.25]));
        assert!(!sender.try_send(&[0.5]));
    }

    #[test]
    fn output_state_underrun_yields_silence() {
        let (_sender, rx, generation) = test_sender(1);
        let mut state = PreviewOutputState::new(rx, generation, INPUT_SAMPLE_RATE);

        assert_eq!(state.next_sample(), 0.0);
    }

    #[test]
    fn clear_discards_buffered_and_queued_audio() {
        let (sender, rx, generation) = test_sender(4);
        let mut state = PreviewOutputState::new(rx, Arc::clone(&generation), INPUT_SAMPLE_RATE);

        assert!(sender.try_send(&[0.5, 0.4, 0.3]));
        assert_eq!(state.next_sample(), 0.5);
        sender.clear();
        assert!(sender.try_send(&[0.9]));

        assert_eq!(state.next_sample(), 0.9);
        assert_eq!(state.next_sample(), 0.0);
    }

    #[test]
    fn sender_rejects_empty_and_oversized_frames() {
        let (sender, _rx, _generation) = test_sender(2);

        assert!(!sender.try_send(&[]));
        assert!(!sender.try_send(&vec![0.0; MAX_PREVIEW_FRAME_SAMPLES + 1]));
    }
}

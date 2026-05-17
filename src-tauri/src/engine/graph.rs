//! Per-frame routing logic for the live audio engine.
//!
//! `route_frame` is the single per-frame decision point: it reads the current
//! `VirtualMicMode` plus the optional original-audio tap and dispatches the
//! captured 48 kHz mono PCM frame
//! appropriately:
//!
//! - `PassThrough`: write raw mic audio to the virtual mic ring and measure
//!   output level.
//! - `Translate`: resample 48 kHz → 24 kHz, convert to PCM16, repacketize into
//!   fixed 40 ms OpenAI uplink chunks, and `try_send` into the uplink channel
//!   to the OpenAI transport. The raw mic frame is NEVER written to the ring in
//!   this mode (no-leak guarantee).
//! - `Translate` with original voice percent > 0 additionally taps the 48 kHz
//!   mono frame into the shared `original_queue` (bounded VecDeque) so the pull
//!   task can later mix the delayed original under the translated audio.
//! - `Silence` (unexpected while capture is running): drop frame.
//!
//! Keeping this function pure-ish (no I/O beyond the ring write) lets it be
//! unit-tested without a real shared-memory ring.
//!
//! # Uplink channel
//!
//! The graph loop holds an `Arc<parking_lot::Mutex<Option<Sender<Vec<i16>>>>>`.
//! When the OpenAI session is active the Engine writes a `Sender` into the
//! slot; when the session stops it clears the slot.  The graph loop clones the
//! sender (under the lock, sub-microsecond) on every `mic_to_openai` frame and
//! calls `try_send` — non-blocking, never grows latency.
//!
//! # Original-audio tap (Task 4.3)
//!
//! In `Translate` mode, the graph loop also pushes the 48 kHz mono frame into
//! an `Option<SharedOriginalQueue>` when original voice percent is positive.
//! The queue is `None` at 0%, and `Some(queue)` when original voice should be
//! mixed underneath the translation. The pull task drains 480 samples per tick.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use intervox_core::audio::level_meter::LevelMeter;
use intervox_core::audio::resampler::LinearResampler;
use intervox_core::state::VirtualMicMode;
use intervox_core::FrameRouting;

use super::ring::RingProducer;
use super::translate_chain::{
    push_original_samples, SharedOriginalQueue, OPENAI_UPLINK_CHUNK_SAMPLES,
};
use super::AudioBackpressureCounters;

/// Shared uplink slot type alias — mirrors the one in `engine/mod.rs`.
type UplinkSlot = Arc<parking_lot::Mutex<Option<tokio::sync::mpsc::Sender<Vec<i16>>>>>;

/// Fixed-size uplink packetizer for OpenAI Realtime audio.
///
/// CPAL/CoreAudio callback sizes vary by device and system state.  The realtime
/// transport should not inherit those callback boundaries, because tiny or uneven
/// websocket messages waste CPU and make latency less predictable.  This chunker
/// accepts 24 kHz mono PCM16 samples and emits exactly 40 ms chunks.
pub(super) struct OpenAiChunker {
    pending: Vec<i16>,
}

impl OpenAiChunker {
    pub fn new() -> Self {
        Self {
            pending: Vec::with_capacity(OPENAI_UPLINK_CHUNK_SAMPLES),
        }
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }

    #[cfg(test)]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn push<F>(&mut self, mut samples: &[i16], mut emit: F)
    where
        F: FnMut(Vec<i16>),
    {
        while !samples.is_empty() {
            let remaining = OPENAI_UPLINK_CHUNK_SAMPLES - self.pending.len();
            let take = remaining.min(samples.len());
            self.pending.extend_from_slice(&samples[..take]);
            samples = &samples[take..];

            if self.pending.len() == OPENAI_UPLINK_CHUNK_SAMPLES {
                let chunk = std::mem::replace(
                    &mut self.pending,
                    Vec::with_capacity(OPENAI_UPLINK_CHUNK_SAMPLES),
                );
                emit(chunk);
            }
        }
    }
}

/// Runtime dependencies for routing one captured frame.
pub(super) struct RouteFrameContext<'a> {
    /// Shared ring producer (written only in PassThrough).
    pub ring: &'a RingProducer,
    /// Shared output RMS (written in PassThrough; cleared otherwise).
    pub out_level: &'a AtomicU32,
    /// Shared slot for the OpenAI uplink `Sender`.
    pub uplink_slot: &'a UplinkSlot,
    /// Persistent streaming resampler (48 kHz → 24 kHz).
    pub resampler: &'a mut LinearResampler,
    /// Persistent 24 kHz PCM16 chunker that emits fixed 40 ms OpenAI packets.
    pub uplink_chunker: &'a mut OpenAiChunker,
    /// Optional 48 kHz original-audio tap for Translate with original voice.
    pub original_queue: Option<&'a SharedOriginalQueue>,
    /// Shared counters for lossy realtime backpressure paths.
    pub backpressure: &'a AudioBackpressureCounters,
}

/// Route a single 48 kHz mono `frame` according to the current `mode`.
///
/// Returns `true` only when a fixed 40 ms OpenAI uplink chunk was successfully
/// enqueued to the realtime transport.
pub(super) fn route_frame(mode: VirtualMicMode, frame: &[f32], ctx: RouteFrameContext<'_>) -> bool {
    let original_voice_percent = u32::from(ctx.original_queue.is_some());
    let routing = FrameRouting::for_mode_and_mix(mode, original_voice_percent);

    if routing.mic_to_ring {
        ctx.uplink_chunker.clear();
        // PassThrough: write raw mic audio to the virtual mic ring.  This is a
        // live path, so it must drop stale unread backlog rather than queueing
        // seconds of delayed microphone audio.
        ctx.ring.write_live(frame);
        let level = LevelMeter::measure(frame);
        ctx.out_level.store(level.rms.to_bits(), Ordering::Relaxed);
        false
    } else if routing.mic_to_openai {
        // Translate: do NOT leak raw mic to ring.
        //
        // 1. Resample 48 kHz → 24 kHz (streaming; resampler carries phase
        //    across frames so there are no discontinuities at chunk boundaries).
        // 2. Convert f32 → PCM16 little-endian.
        // 3. Repacketize into fixed 40 ms chunks.
        // 4. Non-blocking try_send into the uplink channel.
        //    - Drops on full (never block, never grow latency).
        //    - No-op when slot is empty (no active OpenAI session).
        let maybe_tx = ctx.uplink_slot.lock().clone();
        let Some(tx) = maybe_tx else {
            ctx.backpressure.uplink_no_session_drop();
            ctx.uplink_chunker.clear();
            ctx.out_level.store(0, Ordering::Relaxed);
            return false;
        };

        let down = ctx.resampler.process(frame);
        let pcm = intervox_core::audio::pcm::f32_to_pcm16(&down);

        let mut sent_to_openai = false;
        ctx.uplink_chunker.push(&pcm, |chunk| {
            // try_send is callable from a blocking thread (it does not .await).
            // Drop on full — never block, never grow latency.
            if tx.try_send(chunk).is_ok() {
                ctx.backpressure.uplink_chunk_sent();
                sent_to_openai = true;
            } else {
                ctx.backpressure.uplink_queue_drop();
            }
        });

        // When original voice percent is positive, also push the SAME 48 kHz
        // mono frame into the original-audio tap queue so the pull task can
        // delay and mix it. At 0%, the queue is None and no original samples
        // are retained.
        if routing.mix_original {
            if let Some(q) = ctx.original_queue {
                push_original_samples(q, frame);
            }
        }

        // out_level stays 0 in translate modes until the pull task writes
        // translated audio into the ring and measures its level instead.
        ctx.out_level.store(0, Ordering::Relaxed);
        sent_to_openai
    } else {
        ctx.uplink_chunker.clear();
        // Silence while capture is running: drop frame.
        ctx.out_level.store(0, Ordering::Relaxed);
        false
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use crate::engine::translate_chain;

    use intervox_core::audio::resampler::LinearResampler;
    use intervox_core::state::VirtualMicMode;

    /// A minimal stub that captures whether `write` was called and how many
    /// frames were written, without touching any real shared memory.
    struct FakeRing {
        written: std::sync::Mutex<Vec<f32>>,
    }

    impl FakeRing {
        fn new() -> Self {
            Self {
                written: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn write(&self, frames: &[f32]) -> usize {
            let mut w = self.written.lock().unwrap();
            w.extend_from_slice(frames);
            frames.len()
        }

        fn was_written(&self) -> bool {
            !self.written.lock().unwrap().is_empty()
        }
    }

    /// Replacement for `RingProducer` in unit tests: mirrors only the methods
    /// called by `route_frame` and does NOT touch POSIX shm.
    struct TestRingProducer(FakeRing);

    impl TestRingProducer {
        fn new() -> Self {
            Self(FakeRing::new())
        }

        fn write(&self, frames: &[f32]) -> usize {
            self.0.write(frames)
        }

        fn was_written(&self) -> bool {
            self.0.was_written()
        }
    }

    type TestUplinkSlot = Arc<parking_lot::Mutex<Option<tokio::sync::mpsc::Sender<Vec<i16>>>>>;

    // A pure routing helper that mirrors `route_frame` but takes a
    // `TestRingProducer` instead of a real `RingProducer` so no shm is needed.
    #[allow(clippy::too_many_arguments)]
    fn route_frame_test(
        mode: VirtualMicMode,
        frame: &[f32],
        ring: &TestRingProducer,
        out_level: &AtomicU32,
        uplink_slot: &TestUplinkSlot,
        resampler: &mut LinearResampler,
        uplink_chunker: &mut super::OpenAiChunker,
        original_queue: Option<&translate_chain::SharedOriginalQueue>,
    ) -> bool {
        use intervox_core::audio::level_meter::LevelMeter;
        use intervox_core::FrameRouting;

        let original_voice_percent = u32::from(original_queue.is_some());
        let routing = FrameRouting::for_mode_and_mix(mode, original_voice_percent);

        if routing.mic_to_ring {
            uplink_chunker.clear();
            ring.write(frame);
            let level = LevelMeter::measure(frame);
            out_level.store(level.rms.to_bits(), Ordering::Relaxed);
            false
        } else if routing.mic_to_openai {
            let maybe_tx = uplink_slot.lock().clone();
            let Some(tx) = maybe_tx else {
                uplink_chunker.clear();
                out_level.store(0, Ordering::Relaxed);
                return false;
            };

            let down = resampler.process(frame);
            let pcm = intervox_core::audio::pcm::f32_to_pcm16(&down);
            let mut sent = false;
            uplink_chunker.push(&pcm, |chunk| {
                if tx.try_send(chunk).is_ok() {
                    sent = true;
                }
            });
            if routing.mix_original {
                if let Some(q) = original_queue {
                    translate_chain::push_original_samples(q, frame);
                }
            }
            out_level.store(0, Ordering::Relaxed);
            sent
        } else {
            uplink_chunker.clear();
            out_level.store(0, Ordering::Relaxed);
            false
        }
    }

    /// A non-zero 10-sample frame for tests that need audio signal.
    fn nonzero_frame() -> Vec<f32> {
        vec![0.1, -0.2, 0.3, -0.1, 0.5, -0.4, 0.2, 0.6, -0.3, 0.1]
    }

    fn empty_uplink() -> TestUplinkSlot {
        Arc::new(parking_lot::Mutex::new(None))
    }

    #[test]
    fn openai_chunker_emits_only_fixed_40ms_chunks() {
        let mut chunker = super::OpenAiChunker::new();
        let mut chunks = Vec::new();

        chunker.push(&vec![1; 400], |chunk| chunks.push(chunk));
        assert!(chunks.is_empty(), "partial chunks must not be emitted");
        assert_eq!(chunker.pending_len(), 400);

        chunker.push(&vec![2; 560], |chunk| chunks.push(chunk));
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            chunks[0].len(),
            translate_chain::OPENAI_UPLINK_CHUNK_SAMPLES
        );
        assert_eq!(chunker.pending_len(), 0);

        chunker.push(
            &vec![3; translate_chain::OPENAI_UPLINK_CHUNK_SAMPLES * 2 + 7],
            |chunk| {
                chunks.push(chunk);
            },
        );
        assert_eq!(chunks.len(), 3);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.len() == translate_chain::OPENAI_UPLINK_CHUNK_SAMPLES));
        assert_eq!(chunker.pending_len(), 7);

        chunker.clear();
        assert_eq!(chunker.pending_len(), 0);
    }

    #[test]
    fn passthrough_writes_to_ring_and_sets_out_level() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(0);
        let frame = nonzero_frame();
        let mut resampler = LinearResampler::new(48_000, 24_000);
        let mut chunker = super::OpenAiChunker::new();

        route_frame_test(
            VirtualMicMode::PassThrough,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            &mut chunker,
            None,
        );

        assert!(ring.was_written(), "PassThrough must write frame to ring");
        let rms = f32::from_bits(out_level.load(Ordering::Relaxed));
        assert!(
            rms > 0.0,
            "PassThrough must produce non-zero out_level, got {rms}"
        );
    }

    #[test]
    fn translate_does_not_write_to_ring_and_clears_out_level() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(u32::MAX); // pre-set to non-zero
        let frame = nonzero_frame();
        let mut resampler = LinearResampler::new(48_000, 24_000);
        let mut chunker = super::OpenAiChunker::new();

        route_frame_test(
            VirtualMicMode::Translate,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            &mut chunker,
            None,
        );

        assert!(
            !ring.was_written(),
            "Translate must NOT write raw mic to ring (no-leak)"
        );
        let rms = f32::from_bits(out_level.load(Ordering::Relaxed));
        assert_eq!(rms, 0.0, "Translate must clear out_level to 0, got {rms}");
    }

    #[test]
    fn translate_original_voice_queue_does_not_write_to_ring_and_clears_out_level() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(u32::MAX);
        let frame = nonzero_frame();
        let mut resampler = LinearResampler::new(48_000, 24_000);
        let mut chunker = super::OpenAiChunker::new();
        let queue = translate_chain::new_original_queue();

        route_frame_test(
            VirtualMicMode::Translate,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            &mut chunker,
            Some(&queue),
        );

        assert!(
            !ring.was_written(),
            "Translate with original queue must NOT write raw mic to ring"
        );
        let rms = f32::from_bits(out_level.load(Ordering::Relaxed));
        assert_eq!(
            rms, 0.0,
            "Translate with original queue must clear out_level to 0"
        );
    }

    #[test]
    fn silence_does_not_write_to_ring_and_clears_out_level() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(u32::MAX);
        let frame = nonzero_frame();
        let mut resampler = LinearResampler::new(48_000, 24_000);
        let mut chunker = super::OpenAiChunker::new();

        route_frame_test(
            VirtualMicMode::Silence,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            &mut chunker,
            None,
        );

        assert!(!ring.was_written(), "Silence must NOT write to ring");
        let rms = f32::from_bits(out_level.load(Ordering::Relaxed));
        assert_eq!(rms, 0.0, "Silence must clear out_level to 0");
    }

    #[test]
    fn passthrough_zero_frame_still_writes_but_rms_is_zero() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(u32::MAX);
        let frame = vec![0.0f32; 480]; // silence frame
        let mut resampler = LinearResampler::new(48_000, 24_000);
        let mut chunker = super::OpenAiChunker::new();

        route_frame_test(
            VirtualMicMode::PassThrough,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            &mut chunker,
            None,
        );

        assert!(
            ring.was_written(),
            "PassThrough must write even a zero frame"
        );
        let rms = f32::from_bits(out_level.load(Ordering::Relaxed));
        assert_eq!(rms, 0.0, "zero frame must produce rms=0.0, got {rms}");
    }

    #[test]
    fn translate_sends_fixed_40ms_pcm16_chunks_to_uplink() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(0);
        // 480-sample 48 kHz frame — resampled to 240 24 kHz samples.
        // Four such frames make one fixed 40 ms OpenAI uplink chunk.
        let frame: Vec<f32> = (0..480).map(|i| (i as f32 / 480.0) * 0.5).collect();
        let mut resampler = LinearResampler::new(48_000, 24_000);
        let mut chunker = super::OpenAiChunker::new();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<i16>>(16);
        let uplink: TestUplinkSlot = Arc::new(parking_lot::Mutex::new(Some(tx)));

        for i in 0..4 {
            let sent = route_frame_test(
                VirtualMicMode::Translate,
                &frame,
                &ring,
                &out_level,
                &uplink,
                &mut resampler,
                &mut chunker,
                None,
            );
            assert_eq!(
                sent,
                i == 3,
                "only the fourth 10 ms frame completes a 40 ms chunk"
            );
        }

        assert!(
            !ring.was_written(),
            "Translate must NOT write raw mic to ring"
        );

        let received = rx
            .try_recv()
            .expect("uplink should have received one chunk");
        assert_eq!(
            received.len(),
            translate_chain::OPENAI_UPLINK_CHUNK_SAMPLES,
            "OpenAI uplink chunk must be exactly 40 ms at 24 kHz"
        );
        assert!(rx.try_recv().is_err(), "only one chunk should be sent");
    }

    #[test]
    fn translate_drops_frame_when_uplink_slot_is_empty() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(0);
        let frame = nonzero_frame();
        let mut resampler = LinearResampler::new(48_000, 24_000);
        let mut chunker = super::OpenAiChunker::new();

        // No sender in the slot — must not panic.
        route_frame_test(
            VirtualMicMode::Translate,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            &mut chunker,
            None,
        );

        assert!(!ring.was_written());
        let rms = f32::from_bits(out_level.load(Ordering::Relaxed));
        assert_eq!(rms, 0.0);
        assert_eq!(
            chunker.pending_len(),
            0,
            "no OpenAI session means no pre-session audio may accumulate"
        );
    }

    /// Verify that `route_frame` (the real one using `RingProducer`) compiles
    /// and the signature is correct.  Only checks types — not called at runtime.
    #[allow(dead_code)]
    fn _type_check_route_frame_signature(
        ring: &super::RingProducer,
        out_level: &AtomicU32,
        uplink: &super::UplinkSlot,
        resampler: &mut LinearResampler,
    ) {
        let mut chunker = super::OpenAiChunker::new();
        super::route_frame(
            VirtualMicMode::PassThrough,
            &[],
            super::RouteFrameContext {
                ring,
                out_level,
                uplink_slot: uplink,
                resampler,
                uplink_chunker: &mut chunker,
                original_queue: None,
                backpressure: &super::AudioBackpressureCounters::default(),
            },
        );
    }

    // ── Task 4.3: original-queue tap tests in graph context ───────────────────

    #[test]
    fn translate_with_positive_original_percent_pushes_to_original_queue() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(0);
        let frame: Vec<f32> = vec![0.5f32; translate_chain::PULL_FRAMES];
        let mut resampler = LinearResampler::new(48_000, 24_000);
        let mut chunker = super::OpenAiChunker::new();
        let queue = translate_chain::new_original_queue();
        let (tx, _rx) = tokio::sync::mpsc::channel::<Vec<i16>>(16);
        let uplink: TestUplinkSlot = Arc::new(parking_lot::Mutex::new(Some(tx)));

        route_frame_test(
            VirtualMicMode::Translate,
            &frame,
            &ring,
            &out_level,
            &uplink,
            &mut resampler,
            &mut chunker,
            Some(&queue),
        );

        let q_len = queue.lock().len();
        assert!(
            q_len > 0,
            "Translate with original voice must push frames to original_queue; got len={q_len}"
        );
        assert!(
            !ring.was_written(),
            "Translate with original voice must NOT write raw mic to ring"
        );
    }

    #[test]
    fn translate_zero_original_percent_does_not_push_to_original_queue() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(0);
        let frame: Vec<f32> = vec![0.5f32; translate_chain::PULL_FRAMES];
        let mut resampler = LinearResampler::new(48_000, 24_000);
        let mut chunker = super::OpenAiChunker::new();
        let queue = translate_chain::new_original_queue();

        route_frame_test(
            VirtualMicMode::Translate,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            &mut chunker,
            None,
        );

        let q_len = queue.lock().len();
        assert_eq!(
            q_len, 0,
            "Translate at 0% original voice must NOT push to original_queue; got len={q_len}"
        );
    }
}

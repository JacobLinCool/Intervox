//! Per-frame routing logic for the live audio engine.
//!
//! `route_frame` is the single per-frame decision point: it reads the current
//! `VirtualMicMode` and dispatches the captured 48 kHz mono PCM frame
//! appropriately:
//!
//! - `PassThrough`: write raw mic audio to the virtual mic ring and measure
//!   output level.
//! - `Translate` / `TranslateWithOriginal`: resample 48 kHz → 24 kHz, convert
//!   to PCM16, and `try_send` into the uplink channel to the OpenAI transport.
//!   The raw mic frame is NEVER written to the ring in these modes (no-leak
//!   guarantee).
//! - `TranslateWithOriginal` additionally taps the 48 kHz mono frame into the
//!   shared `original_queue` (bounded VecDeque) so the pull task can later mix
//!   the delayed original under the translated audio (Task 4.3).
//! - `Silence` (defensive — capture should not be running): drop frame.
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
//! In `TranslateWithOriginal` mode, the graph loop also pushes the 48 kHz mono
//! frame into an `Option<SharedOriginalQueue>`.  The queue is `None` when the
//! mode is `Translate` (no original), `Some(queue)` when the mode is
//! `TranslateWithOriginal`.  The pull task drains 480 samples per tick.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use intervox_core::audio::level_meter::LevelMeter;
use intervox_core::audio::resampler::LinearResampler;
use intervox_core::state::VirtualMicMode;
use intervox_core::FrameRouting;

use super::ring::RingProducer;
use super::translate_chain::{push_original_samples, SharedOriginalQueue};

/// Shared uplink slot type alias — mirrors the one in `engine/mod.rs`.
type UplinkSlot = Arc<parking_lot::Mutex<Option<tokio::sync::mpsc::Sender<Vec<i16>>>>>;

/// Route a single 48 kHz mono `frame` according to the current `mode`.
///
/// # Parameters
/// - `mode`: current engine mode (read lock-free from `mode_atomic`).
/// - `frame`: 48 kHz mono f32 samples from the CPAL capture thread.
/// - `ring`: shared ring producer (written only in PassThrough).
/// - `out_level`: shared output RMS (written in PassThrough; cleared otherwise).
/// - `uplink_slot`: shared slot for the OpenAI uplink `Sender`.
/// - `resampler`: persistent streaming resampler (48 kHz → 24 kHz).
///   Caller must create one `LinearResampler::new(48_000, 24_000)` and reuse
///   it across frames so phase state carries across chunk boundaries.
/// - `original_queue`: optional shared queue for the 48 kHz original-audio tap
///   (Task 4.3).  `Some(queue)` when mode is `TranslateWithOriginal`; `None`
///   for `Translate` (original not needed) and all other modes.
///   In `TranslateWithOriginal`, the SAME 48 kHz mono frame that goes up the
///   uplink is also pushed here so the pull task can mix delayed original under
///   the translated audio.  No leak: only this queue is written, not the ring.
pub fn route_frame(
    mode: VirtualMicMode,
    frame: &[f32],
    ring: &RingProducer,
    out_level: &AtomicU32,
    uplink_slot: &UplinkSlot,
    resampler: &mut LinearResampler,
    original_queue: Option<&SharedOriginalQueue>,
) {
    let routing = FrameRouting::for_mode(mode);

    if routing.mic_to_ring {
        // PassThrough: write raw mic audio to the virtual mic ring.
        ring.write(frame);
        let level = LevelMeter::measure(frame);
        out_level.store(level.rms.to_bits(), Ordering::Relaxed);
    } else if routing.mic_to_openai {
        // Translate / TranslateWithOriginal: do NOT leak raw mic to ring.
        //
        // 1. Resample 48 kHz → 24 kHz (streaming; resampler carries phase
        //    across frames so there are no discontinuities at chunk boundaries).
        // 2. Convert f32 → PCM16 little-endian.
        // 3. Non-blocking try_send into the uplink channel.
        //    - Drops on full (never block, never grow latency).
        //    - No-op when slot is empty (no active OpenAI session).
        let down = resampler.process(frame);
        let pcm = intervox_core::audio::pcm::f32_to_pcm16(&down);

        // Read the sender from the slot — lock time is sub-microsecond.
        let maybe_tx = uplink_slot.lock().clone();
        if let Some(tx) = maybe_tx {
            // try_send is callable from a blocking thread (it does not .await).
            // Drop on full — never block, never grow latency.
            let _ = tx.try_send(pcm);
        }

        // Task 4.3: when in TranslateWithOriginal (mix_original == true),
        // also push the SAME 48 kHz mono frame into the original-audio tap
        // queue so the pull task can delay and mix it.
        // This is ONLY done when the queue is provided (mix_original path).
        // Translate mode passes None so the push is skipped entirely.
        if routing.mix_original {
            if let Some(q) = original_queue {
                push_original_samples(q, frame);
            }
        }

        // out_level stays 0 in translate modes until the pull task writes
        // translated audio into the ring and measures its level instead.
        out_level.store(0, Ordering::Relaxed);
    } else {
        // Silence (defensive): drop frame.
        out_level.store(0, Ordering::Relaxed);
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

    type TestUplinkSlot =
        Arc<parking_lot::Mutex<Option<tokio::sync::mpsc::Sender<Vec<i16>>>>>;

    // A pure routing helper that mirrors `route_frame` but takes a
    // `TestRingProducer` instead of a real `RingProducer` so no shm is needed.
    fn route_frame_test(
        mode: VirtualMicMode,
        frame: &[f32],
        ring: &TestRingProducer,
        out_level: &AtomicU32,
        uplink_slot: &TestUplinkSlot,
        resampler: &mut LinearResampler,
        original_queue: Option<&translate_chain::SharedOriginalQueue>,
    ) {
        use intervox_core::audio::level_meter::LevelMeter;
        use intervox_core::FrameRouting;

        let routing = FrameRouting::for_mode(mode);

        if routing.mic_to_ring {
            ring.write(frame);
            let level = LevelMeter::measure(frame);
            out_level.store(level.rms.to_bits(), Ordering::Relaxed);
        } else if routing.mic_to_openai {
            let down = resampler.process(frame);
            let pcm = intervox_core::audio::pcm::f32_to_pcm16(&down);
            let maybe_tx = uplink_slot.lock().clone();
            if let Some(tx) = maybe_tx {
                let _ = tx.try_send(pcm);
            }
            if routing.mix_original {
                if let Some(q) = original_queue {
                    translate_chain::push_original_samples(q, frame);
                }
            }
            out_level.store(0, Ordering::Relaxed);
        } else {
            out_level.store(0, Ordering::Relaxed);
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
    fn passthrough_writes_to_ring_and_sets_out_level() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(0);
        let frame = nonzero_frame();
        let mut resampler = LinearResampler::new(48_000, 24_000);

        route_frame_test(
            VirtualMicMode::PassThrough,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            None,
        );

        assert!(ring.was_written(), "PassThrough must write frame to ring");
        let rms = f32::from_bits(out_level.load(Ordering::Relaxed));
        assert!(rms > 0.0, "PassThrough must produce non-zero out_level, got {rms}");
    }

    #[test]
    fn translate_does_not_write_to_ring_and_clears_out_level() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(u32::MAX); // pre-set to non-zero
        let frame = nonzero_frame();
        let mut resampler = LinearResampler::new(48_000, 24_000);

        route_frame_test(
            VirtualMicMode::Translate,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            None,
        );

        assert!(!ring.was_written(), "Translate must NOT write raw mic to ring (no-leak)");
        let rms = f32::from_bits(out_level.load(Ordering::Relaxed));
        assert_eq!(rms, 0.0, "Translate must clear out_level to 0, got {rms}");
    }

    #[test]
    fn translate_with_original_does_not_write_to_ring_and_clears_out_level() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(u32::MAX);
        let frame = nonzero_frame();
        let mut resampler = LinearResampler::new(48_000, 24_000);

        route_frame_test(
            VirtualMicMode::TranslateWithOriginal,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            None,
        );

        assert!(!ring.was_written(), "TranslateWithOriginal must NOT write raw mic to ring");
        let rms = f32::from_bits(out_level.load(Ordering::Relaxed));
        assert_eq!(rms, 0.0, "TranslateWithOriginal must clear out_level to 0");
    }

    #[test]
    fn silence_does_not_write_to_ring_and_clears_out_level() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(u32::MAX);
        let frame = nonzero_frame();
        let mut resampler = LinearResampler::new(48_000, 24_000);

        route_frame_test(
            VirtualMicMode::Silence,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
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

        route_frame_test(
            VirtualMicMode::PassThrough,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            None,
        );

        assert!(ring.was_written(), "PassThrough must write even a zero frame");
        let rms = f32::from_bits(out_level.load(Ordering::Relaxed));
        assert_eq!(rms, 0.0, "zero frame must produce rms=0.0, got {rms}");
    }

    #[test]
    fn translate_sends_pcm16_to_uplink_when_slot_is_filled() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(0);
        // 480-sample 48 kHz frame — resampled to ~240 24 kHz samples.
        let frame: Vec<f32> = (0..480).map(|i| (i as f32 / 480.0) * 0.5).collect();
        let mut resampler = LinearResampler::new(48_000, 24_000);

        let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<i16>>(16);
        let uplink: TestUplinkSlot = Arc::new(parking_lot::Mutex::new(Some(tx)));

        route_frame_test(
            VirtualMicMode::Translate,
            &frame,
            &ring,
            &out_level,
            &uplink,
            &mut resampler,
            None,
        );

        assert!(!ring.was_written(), "Translate must NOT write raw mic to ring");

        // The PCM16 frame should have been sent.
        let received = rx.try_recv().expect("uplink should have received a frame");
        assert!(!received.is_empty(), "PCM16 frame must not be empty");
        // Should be roughly half the input samples (48k → 24k).
        assert!(
            received.len() > 100 && received.len() < 300,
            "expected ~240 samples after 2x downsample, got {}",
            received.len()
        );
    }

    #[test]
    fn translate_drops_frame_when_uplink_slot_is_empty() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(0);
        let frame = nonzero_frame();
        let mut resampler = LinearResampler::new(48_000, 24_000);

        // No sender in the slot — must not panic.
        route_frame_test(
            VirtualMicMode::Translate,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            None,
        );

        assert!(!ring.was_written());
        let rms = f32::from_bits(out_level.load(Ordering::Relaxed));
        assert_eq!(rms, 0.0);
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
        super::route_frame(
            VirtualMicMode::PassThrough,
            &[],
            ring,
            out_level,
            uplink,
            resampler,
            None,
        );
    }

    // ── Task 4.3: original-queue tap tests in graph context ───────────────────

    #[test]
    fn translate_with_original_pushes_to_original_queue() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(0);
        let frame: Vec<f32> = vec![0.5f32; translate_chain::PULL_FRAMES];
        let mut resampler = LinearResampler::new(48_000, 24_000);
        let queue = translate_chain::new_original_queue();

        route_frame_test(
            VirtualMicMode::TranslateWithOriginal,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            Some(&queue),
        );

        let q_len = queue.lock().len();
        assert!(
            q_len > 0,
            "TranslateWithOriginal must push frames to original_queue; got len={q_len}"
        );
        assert!(
            !ring.was_written(),
            "TranslateWithOriginal must NOT write raw mic to ring"
        );
    }

    #[test]
    fn translate_mode_does_not_push_to_original_queue() {
        let ring = TestRingProducer::new();
        let out_level = AtomicU32::new(0);
        let frame: Vec<f32> = vec![0.5f32; translate_chain::PULL_FRAMES];
        let mut resampler = LinearResampler::new(48_000, 24_000);
        // Even if a queue is passed, Translate mode must not push to it.
        let queue = translate_chain::new_original_queue();

        route_frame_test(
            VirtualMicMode::Translate,
            &frame,
            &ring,
            &out_level,
            &empty_uplink(),
            &mut resampler,
            Some(&queue),
        );

        let q_len = queue.lock().len();
        assert_eq!(
            q_len, 0,
            "Translate mode must NOT push to original_queue (mix_original=false); got len={q_len}"
        );
    }
}

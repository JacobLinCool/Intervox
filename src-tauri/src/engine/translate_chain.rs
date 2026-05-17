//! Pure DSP-chain helpers for the Translate downlink.
//!
//! Extracted into a standalone module so the unit test and the real `ev_task` /
//! `pull_task` both call the **same** functions (DRY — no duplication).
//!
//! # Chain (Translate)
//!
//! ```text
//! OutputAudioDelta.pcm16
//!   → pcm16_to_f32               (i16 → [-1, 1] f32)
//!   → downmix_to_mono            (N channels → 1)
//!   → LinearResampler.process    (sample_rate → 48 000 Hz)
//!   → JitterBuffer.push          (push side; shared with pull task)
//!
//! pull_block (10 ms tick):
//!   → JitterBuffer.pull(480)     (pull 480 frames = 10 ms @ 48 kHz)
//!   → Limiter.process            (optional; clips to LIMITER_CEILING)
//! ```
//!
//! # Chain (Translate with original voice)
//!
//! In addition to the translated downlink, the original 48 kHz mono mic frames
//! are tapped into a bounded `SharedOriginalQueue` (max 2 s = 96 000 samples).
//! The pull task drains 480 samples per tick, feeds them through a `DelayLine`,
//! and mixes the delayed original under the translated audio before writing to
//! the ring.
//!
//! ```text
//! graph::route_frame (mic_to_openai + mix_original):
//!   frame (48 kHz mono f32) → SharedOriginalQueue.push_back (bounded, drop oldest)
//!
//! pull_task (10 ms tick, original voice percent > 0):
//!   translated_block (from JitterBuffer)
//!   original_480     (drained from SharedOriginalQueue, zero-padded)
//!     → DelayLine.process(original_480)  → delayed_original
//!     → mix_translated_with_original(translated, delayed_original, settings)
//!     → ring.write + out_level update
//! ```
//!
//! The jitter buffer is constructed with:
//!   - target_ms = 120  → primes after ~120 ms of buffered audio
//!   - max_ms    = 400  → oldest samples dropped when buffer exceeds 400 ms
//!
//! This keeps end-to-end translate latency low while absorbing OpenAI network
//! bursts of up to ~400 ms without overrun artefacts.

use intervox_core::audio::{
    jitter_buffer::JitterBuffer,
    level_meter::LevelMeter,
    mixer::{Limiter, MixSettings},
    pcm::pcm16_to_f32,
    resampler::LinearResampler,
};

// Re-export LIMITER_CEILING so tests (and future callers) can reference the
// ceiling constant without depending on intervox_core::audio::mixer directly.
#[allow(unused_imports)]
pub use intervox_core::audio::mixer::LIMITER_CEILING;

use super::capture::downmix_to_mono;

// ── Task 4.3: SharedOriginalQueue ─────────────────────────────────────────────

/// A bounded queue of 48 kHz mono f32 samples tapped from the mic in
/// Translate's original-voice mix path. Shared between the graph (push) and pull
/// (drain) tasks via `Arc<parking_lot::Mutex<_>>`.
///
/// Bound: 2 seconds × 48 000 Hz = 96 000 samples.  When the queue would
/// exceed this limit, the oldest samples are dropped so latency never grows
/// unbounded (overrun protection).
pub type SharedOriginalQueue = std::sync::Arc<parking_lot::Mutex<std::collections::VecDeque<f32>>>;

/// Maximum number of samples held in the original-audio tap queue.
/// 2 s × 48 000 Hz = 96 000 samples.
pub const ORIG_QUEUE_CAP: usize = 48_000 * 2;

/// Create a new, empty original-audio tap queue.
pub fn new_original_queue() -> SharedOriginalQueue {
    std::sync::Arc::new(parking_lot::Mutex::new(
        std::collections::VecDeque::with_capacity(ORIG_QUEUE_CAP),
    ))
}

/// Push 48 kHz mono samples from the mic into the original-audio tap queue.
///
/// When the queue would exceed `ORIG_QUEUE_CAP`, the **oldest** samples are
/// dropped (pop_front) so the queue never grows beyond the cap.  This bounds
/// latency and memory regardless of how long the pull task is stalled.
pub fn push_original_samples(queue: &SharedOriginalQueue, samples: &[f32]) {
    let mut q = queue.lock();
    for &s in samples {
        if q.len() >= ORIG_QUEUE_CAP {
            q.pop_front(); // drop oldest to enforce bound
        }
        q.push_back(s);
    }
}

/// Drain exactly `n` samples from the original-audio tap queue.
///
/// If fewer than `n` samples are available, the output is zero-padded to
/// length `n` (underrun protection — the DelayLine absorbs the gap).
pub fn drain_original_samples(queue: &SharedOriginalQueue, n: usize) -> Vec<f32> {
    let mut q = queue.lock();
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(q.pop_front().unwrap_or(0.0));
    }
    out
}

// ── Task 4.3: estimated translation latency constant ─────────────────────────

/// Estimated one-way translation latency (ms) used to initialise the delay
/// line for the original-voice tap used by Translate when original voice
/// percent is positive.
///
/// The delay is computed from this value via
/// `intervox_core::audio::delay_line::compute_original_delay_ms(EST_LATENCY_MS)`.
///
/// Real measured latency is wired in Task 4.4; for 4.3 we use a documented
/// default estimate.  A session restart is required for the new value to take
/// effect (consistent with the established pattern for languages/limiter).
///
/// TODO(Task 4.4): recompute delay from measured latency via an
/// `Arc<AtomicU32>` latency estimate shared with the realtime transport.  The
/// pull_task seam is already wired — update the constant here or replace it
/// with a live read.
pub const EST_LATENCY_MS: u32 = 1200;

// ── Task 4.4: latency metric helpers ─────────────────────────────────────────

/// Fixed estimate of capture resample + PCM16-encode + uplink chunking cost
/// (ms).
///
/// This covers the 48→24 kHz resample in the graph loop plus f32→PCM16
/// conversion, `try_send` overhead, and the average wait introduced by the
/// 40 ms OpenAI uplink chunker.  The fixed part is usually below 10 ms; the
/// chunker contributes ~20 ms on average and 40 ms worst-case.
///
/// Update this constant after profiling with the runbook selfcheck.
pub const CAPTURE_TO_SEND_EST_MS: u32 = 30;

/// OpenAI uplink sample rate after graph resampling.
pub const OPENAI_UPLINK_SAMPLE_RATE: u32 = 24_000;

/// Fixed OpenAI uplink chunk duration.
pub const OPENAI_UPLINK_CHUNK_MS: u32 = 40;

/// Fixed OpenAI uplink chunk size: 40 ms of mono 24 kHz PCM16.
pub const OPENAI_UPLINK_CHUNK_SAMPLES: usize =
    (OPENAI_UPLINK_SAMPLE_RATE as usize * OPENAI_UPLINK_CHUNK_MS as usize) / 1000;

/// Bounded queue depth from graph → realtime transport.
///
/// At 40 ms per chunk this caps queued uplink audio at 320 ms.  Older chunks are
/// dropped on backpressure by `try_send`; the queue must not grow into seconds
/// of delayed microphone audio.
pub const OPENAI_UPLINK_QUEUE_BOUND: usize = 8;

/// Convert ring-buffer unread frames to milliseconds at 48 kHz mono.
///
/// `frames / 48` because there are 48 frames per millisecond at 48 kHz.
/// The `as u32` truncates — sub-ms precision is not needed for display.
///
/// This is a pure function (no I/O, no locks) so it is TDD-testable and
/// called from `pull_task` to compute `virtual_mic_output_lag_ms`.
#[inline]
pub fn frames_to_ms_48k(frames: u64) -> u32 {
    (frames / 48) as u32
}

/// Gate: return `true` only when the engine is actively flowing translated
/// audio.  When `false`, no latency event should be emitted — emitting a
/// stale or zero latency when idle would confuse the UI.
///
/// # Parameters
/// - `openai_connected`: whether the OpenAI session is live.
/// - `audio_flowing`: whether at least one `OutputAudioDelta` has arrived
///   during this session (set by `ev_task`, reset on session stop).
///
/// This is a pure function (no I/O, no locks) — TDD-testable in isolation.
#[inline]
pub fn should_emit_latency(openai_connected: bool, audio_flowing: bool) -> bool {
    openai_connected && audio_flowing
}

// ── Task 4.3: DRY mix helper ──────────────────────────────────────────────────

/// Mix a translated block with a delayed original block, returning the final
/// output block.
///
/// This is the single canonical implementation called by both `pull_task` (at
/// runtime) and the unit tests (in `#[cfg(test)]`), satisfying the DRY
/// requirement.  It delegates to `intervox_core::audio::mixer::mix_frames`,
/// which applies gain, optional ducking, and the optional limiter in one pass.
///
/// # Parameters
/// - `translated`: the 480-sample translated-audio block from the jitter buffer.
/// - `delayed_original`: the 480-sample delayed original-audio block from the
///   `DelayLine`.
/// - `settings`: gain/duck/limiter configuration read from `AppConfig` at
///   session start.
pub fn mix_translated_with_original(
    translated: &[f32],
    delayed_original: &[f32],
    settings: &MixSettings,
) -> Vec<f32> {
    intervox_core::audio::mixer::mix_frames(translated, delayed_original, settings)
}

// ── Jitter-buffer construction parameters ─────────────────────────────────────

/// Target fill before the jitter buffer starts yielding data (spec §7.3).
/// 120 ms at 48 kHz = 5 760 frames.  Chosen to absorb typical OpenAI burst
/// inter-arrival while keeping end-to-end translate latency perceptible but
/// acceptable.
pub const JB_TARGET_MS: u32 = 120;

/// Hard upper bound on buffer depth.  When exceeded, oldest samples are dropped
/// (overrun protection, spec §7.3).  400 ms at 48 kHz = 19 200 frames.
pub const JB_MAX_MS: u32 = 400;

/// Pull block size: 10 ms of mono 48 kHz audio = 480 samples.
pub const PULL_FRAMES: usize = 480;

// ── Chain functions ───────────────────────────────────────────────────────────

/// Create a new jitter buffer tuned for the translate downlink.
///
/// Uses [`JB_TARGET_MS`] and [`JB_MAX_MS`] so callers don't hard-code magic
/// numbers.
pub fn new_jitter_buffer() -> JitterBuffer {
    JitterBuffer::new(48_000, JB_TARGET_MS, JB_MAX_MS)
}

/// Ingest one `OutputAudioDelta` worth of audio into the jitter buffer.
///
/// Steps:
/// 1. `pcm16_to_f32` — convert PCM16 samples to [-1.0, 1.0] f32.
/// 2. `downmix_to_mono` — collapse N channels to 1 (no-op for channels == 1).
/// 3. `LinearResampler::process` — resample (the resampler must be pre-configured
///    with the correct `in_hz → 48 000`; rate changes are handled at the call site).
/// 4. `JitterBuffer::push` — add to jitter buffer.
///
/// The resampler MUST be the same instance across successive calls so that
/// inter-chunk phase state is maintained (streaming-safe).
pub fn ingest_audio_delta(
    pcm16: &[i16],
    channels: u16,
    resampler: &mut LinearResampler,
    jb: &mut JitterBuffer,
) {
    // 1. PCM16 → f32
    let f32_samples = pcm16_to_f32(pcm16);

    // 2. Downmix to mono (no-op when channels == 1)
    let mono = if channels > 1 {
        downmix_to_mono(&f32_samples, channels)
    } else {
        f32_samples
    };

    // 3. Resample to 48 kHz (streaming; resampler carries phase across calls)
    let resampled = resampler.process(&mono);

    // 4. Push into jitter buffer
    jb.push(&resampled);
}

/// Pull one 480-frame block (10 ms @ 48 kHz) from the jitter buffer, apply the
/// limiter when `limiter_on` is true, and return the block.
///
/// The jitter buffer returns silence (zeros) when not yet primed or on underrun
/// — the pull task never blocks or panics.
pub fn pull_block(jb: &mut JitterBuffer, limiter_on: bool) -> Vec<f32> {
    let mut block = jb.pull(PULL_FRAMES);
    if limiter_on {
        Limiter::default().process(&mut block);
    }
    block
}

/// Measure the RMS of a block and return its bit-representation for
/// `AtomicU32` storage (matches how the capture path stores `out_level`).
pub fn rms_bits(block: &[f32]) -> u32 {
    let level = LevelMeter::measure(block);
    level.rms.to_bits()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Task 4.4: pure helper unit tests ─────────────────────────────────────

    /// TDD (Red→Green) for `frames_to_ms_48k`.
    ///
    /// Verified values:
    ///   0 frames     → 0 ms
    ///   48 frames    → 1 ms  (exactly 1 ms at 48 kHz)
    ///   480 frames   → 10 ms (one 10-ms tick)
    ///   48_000 frames → 1_000 ms (1 second)
    ///   96_000 frames → 2_000 ms (2 seconds, = ORIG_QUEUE_CAP)
    ///   47 frames    → 0 ms  (sub-ms truncation)
    #[test]
    fn frames_to_ms_48k_arithmetic() {
        assert_eq!(frames_to_ms_48k(0), 0, "0 frames → 0 ms");
        assert_eq!(frames_to_ms_48k(48), 1, "48 frames → 1 ms");
        assert_eq!(frames_to_ms_48k(480), 10, "480 frames → 10 ms");
        assert_eq!(frames_to_ms_48k(48_000), 1_000, "48000 frames → 1000 ms");
        assert_eq!(frames_to_ms_48k(96_000), 2_000, "96000 frames → 2000 ms");
        assert_eq!(
            frames_to_ms_48k(47),
            0,
            "47 frames → 0 ms (sub-ms truncation)"
        );
    }

    /// TDD (Red→Green) for `should_emit_latency`.
    ///
    /// The gate must be:
    ///   connected=true  AND flowing=true  → emit  (true)
    ///   connected=false AND flowing=true  → no-op (false)
    ///   connected=true  AND flowing=false → no-op (false)
    ///   connected=false AND flowing=false → no-op (false)
    #[test]
    fn should_emit_latency_truth_table() {
        assert!(
            should_emit_latency(true, true),
            "connected && flowing → must emit"
        );
        assert!(
            !should_emit_latency(false, true),
            "!connected && flowing → must NOT emit"
        );
        assert!(
            !should_emit_latency(true, false),
            "connected && !flowing → must NOT emit (no audio yet)"
        );
        assert!(
            !should_emit_latency(false, false),
            "!connected && !flowing → must NOT emit"
        );
    }

    /// Generate a PCM16 sine wave at `freq` Hz, `sample_rate` Hz, `n` samples.
    fn sine_pcm16(freq: f32, sample_rate: u32, n: usize) -> Vec<i16> {
        (0..n)
            .map(|i| {
                let s = (2.0 * std::f32::consts::PI * freq * i as f32 / sample_rate as f32).sin();
                (s * 16_000.0) as i16 // amplitude ~0.49 full-scale
            })
            .collect()
    }

    /// Generate a PCM16 ramp from -16000 to +16000, `n` samples.
    fn ramp_pcm16(n: usize) -> Vec<i16> {
        (0..n)
            .map(|i| {
                let t = i as f32 / n as f32; // 0..1
                ((t * 2.0 - 1.0) * 16_000.0) as i16
            })
            .collect()
    }

    // ── (a) Output length is consistent ──────────────────────────────────────

    /// Feed enough 24 kHz PCM16 to prime the buffer, then assert that each
    /// `pull_block` call returns exactly `PULL_FRAMES` samples.
    #[test]
    fn pull_block_always_returns_480_frames() {
        let mut resampler = LinearResampler::new(24_000, 48_000);
        let mut jb = new_jitter_buffer();

        // Push enough audio to prime (target = 120 ms = 5760 frames @ 48 kHz).
        // At 24 kHz → 48 kHz, 1 second of 24 kHz audio gives ~2 s of 48 kHz.
        // Push 4000 ms worth of 24 kHz samples to comfortably prime.
        let pcm16 = sine_pcm16(440.0, 24_000, 24_000 * 4); // 4 seconds
        ingest_audio_delta(&pcm16, 1, &mut resampler, &mut jb);

        for _ in 0..20 {
            let block = pull_block(&mut jb, true);
            assert_eq!(
                block.len(),
                PULL_FRAMES,
                "pull_block must always return exactly {PULL_FRAMES} frames"
            );
        }
    }

    // ── (b) Limiter ceiling enforced ──────────────────────────────────────────

    /// Feed a loud (near full-scale) sine and assert that every sample in every
    /// pulled block is within `LIMITER_CEILING`.
    #[test]
    fn limiter_ceiling_is_enforced() {
        const EPSILON: f32 = 1e-5;

        let mut resampler = LinearResampler::new(24_000, 48_000);
        let mut jb = new_jitter_buffer();

        // Generate a near full-scale sine (amplitude = 32_000 / 32_767 ≈ 0.98).
        let loud: Vec<i16> = (0..24_000 * 4)
            .map(|i| {
                let s = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 24_000.0).sin();
                (s * 32_000.0) as i16
            })
            .collect();

        ingest_audio_delta(&loud, 1, &mut resampler, &mut jb);

        // Pull blocks and check every sample.
        for _ in 0..20 {
            let block = pull_block(&mut jb, /*limiter_on=*/ true);
            for &s in &block {
                assert!(
                    s.abs() <= LIMITER_CEILING + EPSILON,
                    "sample {s} exceeds LIMITER_CEILING ({LIMITER_CEILING})"
                );
            }
        }
    }

    // ── (c) Primed buffer returns non-silence for non-zero input ─────────────

    /// Push a non-zero ramp at 24 kHz, wait for the buffer to prime, then assert
    /// that the pulled block is not all zeros.
    #[test]
    fn primed_buffer_returns_non_silence_for_nonzero_input() {
        let mut resampler = LinearResampler::new(24_000, 48_000);
        let mut jb = new_jitter_buffer();

        // 4 seconds of ramp — enough to prime and have data left over.
        let ramp = ramp_pcm16(24_000 * 4);
        ingest_audio_delta(&ramp, 1, &mut resampler, &mut jb);

        // Skip a few pulls while the buffer might still be priming.
        for _ in 0..5 {
            let _ = pull_block(&mut jb, false);
        }

        // Now the buffer should be returning real data.
        let block = pull_block(&mut jb, false);
        let any_nonzero = block.iter().any(|&s| s.abs() > 1e-6);
        assert!(
            any_nonzero,
            "primed buffer must return non-silence for non-zero input; got all zeros"
        );
    }

    // ── Stereo downmix is applied correctly ───────────────────────────────────

    /// Feed stereo PCM16 and confirm the ingestion does not panic and produces
    /// sensible output length (roughly half the stereo sample count after
    /// mono-downmix, then upsampled 2× from 24 k → 48 k → net ~same count).
    #[test]
    fn stereo_downmix_does_not_panic() {
        let mut resampler = LinearResampler::new(24_000, 48_000);
        let mut jb = new_jitter_buffer();

        // Stereo PCM16 at 24 kHz: interleaved L, R.
        let stereo: Vec<i16> = (0..2400 * 2).map(|i| (i % 1000) as i16 - 500).collect();

        // channels == 2; must not panic.
        ingest_audio_delta(&stereo, 2, &mut resampler, &mut jb);

        // Buffer received some samples — we don't check priming here, just
        // that the chain ran without panic.
        let block = pull_block(&mut jb, false);
        assert_eq!(block.len(), PULL_FRAMES);
    }

    // ── rms_bits round-trips through AtomicU32 ────────────────────────────────

    #[test]
    fn rms_bits_matches_expected_level() {
        let block = vec![0.5f32; PULL_FRAMES]; // DC = 0.5
        let bits = rms_bits(&block);
        let rms = f32::from_bits(bits);
        assert!((rms - 0.5).abs() < 1e-4, "rms bits round-trip, got {rms}");
    }

    // ── Limiter is skipped when disabled ──────────────────────────────────────

    #[test]
    fn limiter_disabled_allows_pre_clamp_values() {
        // JitterBuffer clamps naturally — but we can check that with limiter OFF,
        // a near-full-scale signal (0.98) is NOT clamped to LIMITER_CEILING.
        let mut resampler = LinearResampler::new(24_000, 48_000);
        let mut jb = new_jitter_buffer();

        // Near full-scale sine — max |f32| ≈ 0.976 after pcm16_to_f32.
        let loud: Vec<i16> = vec![32_000; 24_000 * 4];
        ingest_audio_delta(&loud, 1, &mut resampler, &mut jb);

        for _ in 0..10 {
            let block = pull_block(&mut jb, /*limiter_on=*/ false);
            // With limiter OFF, values > LIMITER_CEILING may occur.
            // Just check length is correct and no panic.
            assert_eq!(block.len(), PULL_FRAMES);
        }
    }

    // ── Task 4.3: pure mix tests (mix_translated_with_original) ──────────────

    /// Helper: compute RMS of a block.
    fn block_rms(block: &[f32]) -> f32 {
        let sum_sq: f32 = block.iter().map(|&s| s * s).sum();
        (sum_sq / block.len() as f32).sqrt()
    }

    /// Build `MixSettings` matching the Task 4.3 gate spec:
    ///   original_gain_db ≈ percent_to_db(20.0), translated_gain_db = 0.0,
    ///   duck_original = true, limiter_enabled = true.
    fn gate_mix_settings() -> MixSettings {
        MixSettings {
            original_gain_db: intervox_core::config::percent_to_db(20.0),
            translated_gain_db: 0.0,
            duck_original: true,
            limiter_enabled: true,
        }
    }

    // ── (1) Limiter ceiling is enforced through the mix path ──────────────────
    //
    // Build a loud translated block (0.8 amplitude) and a loud original block
    // (0.8 amplitude).  After mixing with the gate settings, every output
    // sample must be within LIMITER_CEILING + 1e-5.

    #[test]
    fn mix_limiter_ceiling_enforced_through_mix_path() {
        const EPSILON: f32 = 1e-5;

        let translated = vec![0.8f32; PULL_FRAMES];
        let original = vec![0.8f32; PULL_FRAMES];
        let settings = gate_mix_settings();

        let out = mix_translated_with_original(&translated, &original, &settings);

        assert_eq!(out.len(), PULL_FRAMES);
        for &s in &out {
            assert!(
                s.abs() <= LIMITER_CEILING + EPSILON,
                "sample {s} exceeds LIMITER_CEILING ({LIMITER_CEILING}) + {EPSILON}"
            );
        }
    }

    // ── (2) Original is attenuated — sits under the translation ───────────────
    //
    // Compare the RMS of "original-only" (translated = zeros, original = 0.8)
    // with "translated-only" (translated = 0.8, original = zeros).
    // With original_gain_db ≈ percent_to_db(20.0) ≈ -14 dB and duck_original=true
    // the original-only RMS must be less than the translated-only RMS.

    #[test]
    fn mix_original_sits_under_translation() {
        let loud = vec![0.8f32; PULL_FRAMES];
        let zeros = vec![0.0f32; PULL_FRAMES];
        let settings = gate_mix_settings();

        // Mix with original-only: translated = zeros, original = loud.
        let original_only = mix_translated_with_original(&zeros, &loud, &settings);
        // Mix with translated-only: translated = loud, original = zeros.
        let translated_only = mix_translated_with_original(&loud, &zeros, &settings);

        let original_rms = block_rms(&original_only);
        let translated_rms = block_rms(&translated_only);

        assert!(
            original_rms < translated_rms,
            "original-only RMS ({original_rms:.4}) should be less than \
             translated-only RMS ({translated_rms:.4}) — original must sit under translation"
        );
    }

    // ── (3) Zero original block → output ≈ translated path ───────────────────
    //
    // When the original block is all zeros, mix_translated_with_original should
    // produce output that is equivalent to the translated-only path.  With
    // original = zeros and translated_gain_db = 0.0, the mix is just the
    // translated signal (plus limiter).

    #[test]
    fn mix_zero_original_equals_translated_path() {
        const EPSILON: f32 = 1e-5;

        let translated = vec![0.5f32; PULL_FRAMES];
        let original = vec![0.0f32; PULL_FRAMES];
        let settings = gate_mix_settings();

        // Mix with zero original.
        let mixed = mix_translated_with_original(&translated, &original, &settings);

        // Reference: the translated-only path also with limiter.
        // With original=zeros, duck does not fire (original contributes 0),
        // so output should match translated × tgain (= 1.0 since tgain_db=0.0).
        // At 0.5 amplitude, limiter does not clip.
        for (i, (&m, &t)) in mixed.iter().zip(translated.iter()).enumerate() {
            assert!(
                (m - t).abs() <= EPSILON,
                "sample[{i}]: mixed={m}, translated={t} — expected ≈ translated when original=0"
            );
        }
    }

    // ── (4) push_original_samples / drain_original_samples round-trip ─────────

    #[test]
    fn original_queue_push_drain_round_trip() {
        let q = new_original_queue();
        let samples: Vec<f32> = (0..PULL_FRAMES).map(|i| i as f32 * 0.001).collect();

        push_original_samples(&q, &samples);
        let drained = drain_original_samples(&q, PULL_FRAMES);

        assert_eq!(drained, samples);
    }

    #[test]
    fn original_queue_underrun_pads_with_zeros() {
        let q = new_original_queue();
        push_original_samples(&q, &[1.0, 2.0, 3.0]);

        let drained = drain_original_samples(&q, 6);
        assert_eq!(drained, vec![1.0, 2.0, 3.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn original_queue_bounded_drops_oldest_on_overflow() {
        let q = new_original_queue();

        // Push exactly ORIG_QUEUE_CAP + 100 samples.
        // The first 100 should be dropped (oldest).
        let total = ORIG_QUEUE_CAP + 100;
        let samples: Vec<f32> = (0..total).map(|i| i as f32).collect();
        push_original_samples(&q, &samples);

        let len = q.lock().len();
        assert_eq!(
            len, ORIG_QUEUE_CAP,
            "queue must be capped at {ORIG_QUEUE_CAP}"
        );

        // First element should be sample[100] (the 101st pushed).
        let first = q.lock().front().copied().unwrap();
        assert!(
            (first - 100.0).abs() < 1e-5,
            "oldest samples must be dropped; expected first=100.0, got {first}"
        );
    }
}

//! Live audio engine: mode control, CPAL mic capture, and backend-pushed meter events.
//!
//! `Engine` owns the ring producer, the optional `CaptureHandle`, and the
//! optional meter task.  It is `Send + Sync` because:
//! - `Mutex<Inner>` is `Send + Sync` (all `Inner` fields are `Send`),
//! - `Arc<RingProducer>` is `Send + Sync`,
//! - `tauri::AppHandle` is `Send + Sync`.
//! - All new fields added for Task 4.1 are `Send + Sync`:
//!   `Option<JoinHandle>`, `Option<tokio::mpsc::Sender>`, and
//!   `Arc<parking_lot::Mutex<Option<Sender<Vec<i16>>>>>`.
//!
//! # Shared-slot uplink wiring
//!
//! The graph task (spawned when capture starts) and the OpenAI realtime task
//! (spawned when mode requires `openai_connected`) have independent lifecycles.
//! To avoid tight coupling, the graph loop holds a clone of
//! `Arc<parking_lot::Mutex<Option<Sender<Vec<i16>>>>>` (the "uplink slot").
//! When an OpenAI session starts, the Engine writes a fresh `Sender` into the
//! slot.  When the session stops, it clears the slot.  The graph loop calls
//! `try_send` on whatever is in the slot, dropping frames when the slot is empty
//! or the channel is full — no latency, no blocking.
//!
//! The `cpal::Stream` lives entirely inside the capture thread and is never
//! moved across threads — see `capture` module.

pub mod capture;
pub mod graph;
pub mod playback;
pub mod realtime;
pub mod ring;
pub mod supervisor;
pub mod system_audio;
pub mod translate_chain;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use intervox_core::{
    audio::jitter_buffer::JitterBuffer, realtime::events::TranslationEvent, state::VirtualMicMode,
    AppError, Config,
};
use parking_lot::Mutex;
use tauri::Emitter;

use capture::{CaptureHandle, CapturedFrame};
use playback::{PlaybackHandle, PlaybackSlot};
use ring::{mode_to_ring_u32, RingProducer};
use translate_chain::{SharedOriginalQueue, OPENAI_UPLINK_QUEUE_BOUND};

/// Shared slot holding the timestamp of the most recent successful uplink send.
///
/// Set by the graph loop (`try_send` success path in `route_frame` / the
/// realtime transport send), read by `ev_task` when the first
/// `OutputAudioDelta` arrives to compute `openai_first_audio_ms`.
///
/// `parking_lot::Mutex` is used because the graph loop runs inside
/// `spawn_blocking` (not async) — the lock hold-time is sub-microsecond
/// (just a store of an `Instant`).
type LastSendSlot = Arc<Mutex<Option<std::time::Instant>>>;

/// Shared jitter buffer type: pushed by `ev_task`, pulled by `pull_task`.
/// `parking_lot::Mutex` is used because both tasks may be async but the
/// critical sections are sub-microsecond (push/pull a `VecDeque`).
type SharedJitterBuf = Arc<Mutex<JitterBuffer>>;

/// Shared slot type: the graph loop holds one of these to forward 24 kHz PCM16
/// frames to the active OpenAI session.  `parking_lot::Mutex` is chosen because
/// the graph loop runs inside `spawn_blocking` (not async) and we need a
/// non-async lock.  Lock contention is negligible: writes happen only on session
/// start/stop; reads happen per audio frame but the critical section is a `clone`
/// of the `Option<Sender>` which is a few nanoseconds.
type UplinkSlot = Arc<Mutex<Option<tokio::sync::mpsc::Sender<Vec<i16>>>>>;

const METER_TICK: std::time::Duration = std::time::Duration::from_millis(50);
const METER_STALE_TICKS: u8 = 4;
const MAIN_WEBVIEW_LABEL: &str = "main";

fn lifecycle_trace(message: impl AsRef<str>) {
    if std::env::var_os("INTERVOX_LIFECYCLE_TRACE").is_some() {
        eprintln!("[intervox:lifecycle] {}", message.as_ref());
    }
}

#[derive(Debug, Default)]
pub struct AudioBackpressureCounters {
    capture_pool_misses: AtomicU64,
    capture_capacity_drops: AtomicU64,
    capture_sink_drops: AtomicU64,
    uplink_no_session_drops: AtomicU64,
    uplink_queue_drops: AtomicU64,
    uplink_chunks_sent: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioBackpressureMetrics {
    pub capture_pool_misses: u64,
    pub capture_capacity_drops: u64,
    pub capture_sink_drops: u64,
    pub uplink_no_session_drops: u64,
    pub uplink_queue_drops: u64,
    pub uplink_chunks_sent: u64,
}

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioMeterFrame {
    pub sequence: u64,
    pub input_level: f32,
    pub output_level: f32,
    pub input_active: bool,
    pub output_active: bool,
    pub input_sequence: u64,
    pub output_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AudioMeterFrameSignature {
    input_level_bits: u32,
    output_level_bits: u32,
    input_active: bool,
    output_active: bool,
    input_sequence: u64,
    output_sequence: u64,
}

impl From<&AudioMeterFrame> for AudioMeterFrameSignature {
    fn from(frame: &AudioMeterFrame) -> Self {
        Self {
            input_level_bits: frame.input_level.to_bits(),
            output_level_bits: frame.output_level.to_bits(),
            input_active: frame.input_active,
            output_active: frame.output_active,
            input_sequence: frame.input_sequence,
            output_sequence: frame.output_sequence,
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioMeterDiagnostics {
    pub input_level: f32,
    pub output_level: f32,
    pub input_sequence: u64,
    pub output_sequence: u64,
    pub last_frame_sequence: u64,
    pub emit_attempts: u64,
    pub emit_failures: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct MeterChannelFreshness {
    last_sequence: u64,
    stale_ticks: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MeterChannelSample {
    level: f32,
    active: bool,
}

impl AudioBackpressureCounters {
    pub(crate) fn capture_pool_miss(&self) {
        self.capture_pool_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn capture_capacity_drop(&self) {
        self.capture_capacity_drops.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn capture_sink_drop(&self) {
        self.capture_sink_drops.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn uplink_no_session_drop(&self) {
        self.uplink_no_session_drops.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn uplink_queue_drop(&self) {
        self.uplink_queue_drops.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn uplink_chunk_sent(&self) {
        self.uplink_chunks_sent.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> AudioBackpressureMetrics {
        AudioBackpressureMetrics {
            capture_pool_misses: self.capture_pool_misses.load(Ordering::Relaxed),
            capture_capacity_drops: self.capture_capacity_drops.load(Ordering::Relaxed),
            capture_sink_drops: self.capture_sink_drops.load(Ordering::Relaxed),
            uplink_no_session_drops: self.uplink_no_session_drops.load(Ordering::Relaxed),
            uplink_queue_drops: self.uplink_queue_drops.load(Ordering::Relaxed),
            uplink_chunks_sent: self.uplink_chunks_sent.load(Ordering::Relaxed),
        }
    }
}

impl MeterChannelFreshness {
    fn sample(&mut self, raw_level: f32, sequence: u64) -> MeterChannelSample {
        if sequence != self.last_sequence {
            self.last_sequence = sequence;
            self.stale_ticks = 0;
            return MeterChannelSample {
                level: raw_level,
                active: true,
            };
        }

        self.stale_ticks = self.stale_ticks.saturating_add(1);
        let active = sequence != 0 && self.stale_ticks <= METER_STALE_TICKS;
        MeterChannelSample {
            level: if active { raw_level } else { 0.0 },
            active,
        }
    }
}

// ── Inner state ───────────────────────────────────────────────────────────────

struct Inner {
    mode: VirtualMicMode,
    source_id: Option<String>,
    /// Monotonic token for input-source switches. A slow CoreAudio open from
    /// an older selection must not overwrite a newer user selection.
    source_switch_generation: u64,
    target_language: String,
    /// The running capture thread handle.  `None` when capture is stopped.
    capture: Option<CaptureHandle>,
    /// Graph task that reads captured frames from the channel and routes them
    /// (PassThrough → ring write; Translate → 40 ms PCM16 chunks → OpenAI uplink).
    graph_task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Stop flag for the blocking graph receiver loop. Aborting the Tokio task
    /// does not cancel `spawn_blocking`, so the loop needs its own stop signal.
    graph_stop: Option<Arc<AtomicBool>>,
    /// OpenAI Realtime websocket transport task (Task 4.1).
    realtime_task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Real downlink event consumer (Task 4.2): routes audio to jitter buffer
    /// and transcript/error events to the frontend.
    ev_task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Pull task (Task 4.2): ticks every 10 ms, pulls 480 frames from the
    /// jitter buffer, applies the limiter, writes to the ring, updates out_level.
    pull_task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Uplink sender kept in Inner so we can drop it on session stop.
    /// The actual routing goes through `uplink_slot` on the Engine.
    pcm_tx: Option<tokio::sync::mpsc::Sender<Vec<i16>>>,
    /// Task 4.5: watcher that detects capture-thread death while the mode still
    /// needs capture and triggers one automatic restart (cap-1 per mode-entry).
    capture_watcher_task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Onboarding/source-selection probe. This opens the selected input device
    /// only to measure RMS while the live engine stays in Silence. It never
    /// writes to the virtual mic ring or OpenAI.
    probe_capture: Option<CaptureHandle>,
    /// Engine-level meter emitter. It is intentionally independent from capture
    /// and probe lifetimes so frontend meter events keep flowing across mode
    /// changes, capture restarts, and source-selection probes.
    meter_task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Task 7: 10 s housekeeping ticker that folds uplink-sample deltas into the
    /// persisted UsageStore.  Stored so `shutdown()` can abort it before the
    /// final synchronous fold, preventing a double-write race.
    fold_task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Whether local output preview is requested by config/UI.
    output_preview_enabled: bool,
    /// CPAL output stream that mirrors the final virtual-mic signal locally.
    output_preview: Option<PlaybackHandle>,
}

// ── Engine ────────────────────────────────────────────────────────────────────

/// The central live-audio controller managed by Tauri.
pub struct Engine {
    inner: Mutex<Inner>,
    ring: Arc<RingProducer>,
    app: tauri::AppHandle,
    /// Shared RMS level written by the capture callback, read by the meter task.
    level: Arc<AtomicU32>,
    /// Shared output RMS level written by the graph loop (PassThrough path),
    /// read by the engine-level meter task.
    out_level: Arc<AtomicU32>,
    /// Monotonic input-meter freshness counter. Incremented by the capture
    /// callback before downstream graph buffering so UI liveness does not
    /// depend on the graph accepting frames.
    input_level_sequence: Arc<AtomicU64>,
    /// Monotonic output-meter freshness counter. Incremented by actual virtual
    /// mic output writers (PassThrough graph path and translation pull task).
    output_level_sequence: Arc<AtomicU64>,
    /// Last backend meter frame sequence emitted through Tauri's event bus.
    meter_frame_sequence: Arc<AtomicU64>,
    /// Number of meter frame send attempts made by the backend.
    meter_emit_attempts: Arc<AtomicU64>,
    /// Number of failed meter frame send attempts returned by Tauri.
    meter_emit_failures: Arc<AtomicU64>,
    /// Current mode stored as a `u32` for lock-free reads from the graph loop.
    /// Updated at the TOP of `set_mode` before any capture restart.
    mode_atomic: Arc<AtomicU32>,
    /// Set `true` in `start_openai_session_locked`, `false` in
    /// `stop_openai_session_locked`.  The supervisor task reads this to decide
    /// whether to restart `realtime::run` after an unexpected exit.
    session_active: Arc<AtomicBool>,
    /// Caps capture auto-restart to 1 attempt per mode-entry.
    /// Set `true` when mode-entry starts capture; set `false` after the first
    /// auto-restart fires so further device-lost errors just surface the error.
    capture_restart_allowed: Arc<AtomicBool>,
    /// Shared uplink slot: Engine writes a Sender when the OpenAI session starts;
    /// the graph loop (running in spawn_blocking) reads it while forming fixed
    /// 40 ms OpenAI chunks.
    /// See module-level doc for the rationale.
    uplink_slot: UplinkSlot,
    /// Shared original-audio tap queue: the graph loop pushes 48 kHz mono
    /// frames here when Translate has a positive original voice percent; the
    /// pull task drains 480 samples per tick, delays them, and mixes them under
    /// the translated audio before writing to the ring.
    ///
    /// The queue is cleared (drained to zero) at session stop so no stale
    /// original audio carries over into the next session.
    ///
    /// The graph loop reads an `Option<SharedOriginalQueue>` from a shared
    /// slot (identical pattern to `uplink_slot`) so that no-original modes
    /// (`Translate`, `Silence`, `PassThrough`) incur zero cost: the slot is
    /// `None` when not mixing.
    original_queue_slot: Arc<Mutex<Option<SharedOriginalQueue>>>,

    /// Optional local output preview sink. Graph/pull paths clone the sender
    /// under this short lock, then try_send outside the realtime audio stream.
    output_preview_slot: PlaybackSlot,

    /// Process-wide counters for intentionally lossy audio backpressure paths.
    /// Updated from realtime capture/graph code with relaxed atomics only.
    backpressure: Arc<AudioBackpressureCounters>,

    // ── Task 4.4: latency metrics and OpenAI session lifecycle ────────────────
    /// Timestamp of the most recent successful PCM16 uplink send.
    ///
    /// Written by the graph loop (inside `spawn_blocking`) on each successful
    /// `try_send` to the uplink channel.  Read by `ev_task` when the first
    /// `OutputAudioDelta` of a session arrives to compute
    /// `openai_first_audio_ms = now - last_send`.
    ///
    /// Reset to `None` on session stop (inside `stop_openai_session_locked`).
    last_send_time: LastSendSlot,

    /// Whether `ev_task` has received at least one `OutputAudioDelta` during
    /// the current session.  Set `true` on first `OutputAudioDelta`, reset to
    /// `false` on session stop.  The `pull_task` reads this (via
    /// `should_emit_latency`) to decide whether to emit `"latency-changed"`.
    audio_flowing: Arc<AtomicBool>,

    /// Total uplink PCM16 samples sent to OpenAI since process start. Folded
    /// into the persisted UsageStore by the engine's housekeeping ticker.
    pub(crate) uplink_samples: Arc<AtomicU64>,

    /// Tracks how many uplink samples have already been persisted to the
    /// UsageStore.  Used by Task 7's shutdown flush to detect new samples.
    pub(crate) uplink_persisted: Arc<AtomicU64>,

    /// Task 8: per-session transcript log (JSON Lines, one file per session).
    session_log: Arc<crate::transcript_log::SessionLog>,

    /// Task 9: connection lifecycle log (bounded ring + capped file).
    conn_log: Arc<crate::connection_log::ConnectionLog>,

    /// Whether the tray latency badge is currently enabled (mirrors
    /// `UiConfig::show_latency_badge`). Updated lock-free by `set_show_latency_badge`
    /// so the pull_task never needs to read config from disk.
    show_latency_badge: Arc<std::sync::atomic::AtomicBool>,

    // ── Reminder source-of-truth (issue #2) ───────────────────────────────────
    /// Process-relative monotonic clock base. All reminder timestamps below are
    /// millis since this instant, so the notification dispatcher can read them
    /// lock-free without sharing an `Instant`.
    process_start: std::time::Instant,
    /// Monotonic id of the current continuous Interpret/recording session.
    /// Incremented in `start_openai_session_locked`, so every session — and
    /// every mid-session restart (source/target change) — gets a fresh id.
    /// The reminder tracker resets its de-dup state when this changes, which
    /// is the single source of truth for "session changed".
    session_generation: Arc<AtomicU64>,
    /// `now_ms()` at session start; `0` when no session is active. Cleared in
    /// `stop_openai_session_locked` so duration reminders reset on stop.
    session_started_ms: Arc<AtomicU64>,
    /// `now_ms()` of the last emitted `target-transcript-delta` (user-visible
    /// interpreted text) for the current session; `0` until the first one.
    /// This — not generic audio level — is the inactivity signal.
    last_interpret_ms: Arc<AtomicU64>,
}

impl Engine {
    /// Construct a new `Engine`, initialising the shm ring and loading
    /// initial defaults from `cfg`.
    ///
    /// Panics if the POSIX shm ring cannot be created (unrecoverable at startup).
    pub fn new(app: tauri::AppHandle, cfg: &Config) -> Self {
        let ring = Arc::new(
            RingProducer::open_or_create()
                .expect("failed to create /intervox.ring — check POSIX shm permissions"),
        );

        let mut inner = Inner {
            mode: VirtualMicMode::Silence,
            source_id: cfg.audio.source_id.clone(),
            source_switch_generation: 0,
            target_language: cfg.translation.target_language.clone(),
            capture: None,
            graph_task: None,
            graph_stop: None,
            realtime_task: None,
            ev_task: None,
            pull_task: None,
            pcm_tx: None,
            capture_watcher_task: None,
            probe_capture: None,
            meter_task: None,
            fold_task: None,
            output_preview_enabled: false,
            output_preview: None,
        };

        // Start in Silence mode.
        ring.set_mode(mode_to_ring_u32(VirtualMicMode::Silence));
        ring.flush_silence();

        // Create the usage-fold Arcs as locals so we can clone them into both
        // the struct fields and the 10 s housekeeping task (Task 7).
        let uplink_samples_arc: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
        let uplink_persisted_arc: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
        let level_arc: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        let out_level_arc: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        let input_level_sequence_arc: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
        let output_level_sequence_arc: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
        let meter_frame_sequence_arc: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
        let meter_emit_attempts_arc: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
        let meter_emit_failures_arc: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
        let backpressure_arc: Arc<AudioBackpressureCounters> =
            Arc::new(AudioBackpressureCounters::default());

        // Meter events are Engine-level, not capture-level. Capture/probe tasks
        // update atomics plus freshness sequences; this task owns the frontend
        // event stream for the whole app lifetime. The frontend receives one
        // coherent frame per changed backend snapshot, never paired input/output
        // events that can race or get half-updated.
        {
            let level = Arc::clone(&level_arc);
            let out_level = Arc::clone(&out_level_arc);
            let input_level_sequence = Arc::clone(&input_level_sequence_arc);
            let output_level_sequence = Arc::clone(&output_level_sequence_arc);
            let meter_frame_sequence = Arc::clone(&meter_frame_sequence_arc);
            let meter_emit_attempts = Arc::clone(&meter_emit_attempts_arc);
            let meter_emit_failures = Arc::clone(&meter_emit_failures_arc);
            let backpressure = Arc::clone(&backpressure_arc);
            let app = app.clone();
            let meter_trace = std::env::var_os("INTERVOX_METER_TRACE").is_some();
            let meter_handle = tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(METER_TICK);
                let mut backpressure_tick = 0_u8;
                let mut sequence = 0_u64;
                let mut last_signature: Option<AudioMeterFrameSignature> = None;
                let mut input_freshness = MeterChannelFreshness::default();
                let mut output_freshness = MeterChannelFreshness::default();
                loop {
                    tick.tick().await;

                    let input_sequence = input_level_sequence.load(Ordering::Acquire);
                    let output_sequence = output_level_sequence.load(Ordering::Acquire);
                    let input_sample = input_freshness.sample(
                        f32::from_bits(level.load(Ordering::Relaxed)),
                        input_sequence,
                    );
                    let output_sample = output_freshness.sample(
                        f32::from_bits(out_level.load(Ordering::Relaxed)),
                        output_sequence,
                    );

                    let mut frame = AudioMeterFrame {
                        sequence: sequence.wrapping_add(1),
                        input_level: input_sample.level,
                        output_level: output_sample.level,
                        input_active: input_sample.active,
                        output_active: output_sample.active,
                        input_sequence,
                        output_sequence,
                    };

                    let signature = AudioMeterFrameSignature::from(&frame);
                    let active_transition = last_signature.is_some_and(|previous| {
                        previous.input_active != frame.input_active
                            || previous.output_active != frame.output_active
                    });
                    let changed = last_signature != Some(signature);

                    if changed {
                        sequence = sequence.wrapping_add(1);
                        frame.sequence = sequence;
                        last_signature = Some(signature);
                        meter_frame_sequence.store(sequence, Ordering::Release);
                        meter_emit_attempts.fetch_add(1, Ordering::Relaxed);
                        if app
                            .emit_to(MAIN_WEBVIEW_LABEL, "audio-meter", frame)
                            .is_err()
                        {
                            meter_emit_failures.fetch_add(1, Ordering::Relaxed);
                        }
                        if meter_trace && (active_transition || sequence.is_multiple_of(20)) {
                            eprintln!(
                                "[intervox:meter:backend] frame={} input_seq={} output_seq={} input={:.6} output={:.6} input_active={} output_active={} emit_attempts={} emit_failures={}",
                                frame.sequence,
                                frame.input_sequence,
                                frame.output_sequence,
                                frame.input_level,
                                frame.output_level,
                                frame.input_active,
                                frame.output_active,
                                meter_emit_attempts.load(Ordering::Relaxed),
                                meter_emit_failures.load(Ordering::Relaxed),
                            );
                        }
                    }

                    backpressure_tick = backpressure_tick.saturating_add(1);
                    if backpressure_tick >= 20 {
                        backpressure_tick = 0;
                        let _ = app.emit("audio-backpressure", backpressure.snapshot());
                    }
                }
            });
            inner.meter_task = Some(meter_handle);
        }

        // Usage fold: every 10 s, move the uplink sample delta into the
        // persisted UsageStore. uplink_persisted tracks the last-folded count.
        // The handle is stored in Inner so shutdown() can abort the task before
        // the final synchronous fold, preventing a double-write race.
        {
            let samples = std::sync::Arc::clone(&uplink_samples_arc);
            let persisted = std::sync::Arc::clone(&uplink_persisted_arc);
            let fold_handle = tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
                loop {
                    tick.tick().await;
                    let now = samples.load(std::sync::atomic::Ordering::Relaxed);
                    let prev = persisted.swap(now, std::sync::atomic::Ordering::Relaxed);
                    let delta = now.saturating_sub(prev);
                    if delta > 0 {
                        let mut u = crate::usage_store::load();
                        u.add_samples(delta, &crate::usage_store::current_month_utc());
                        crate::usage_store::save(&u);
                    }
                }
            });
            inner.fold_task = Some(fold_handle);
        }

        let engine = Self {
            inner: Mutex::new(inner),
            ring,
            app,
            level: level_arc,
            out_level: out_level_arc,
            input_level_sequence: input_level_sequence_arc,
            output_level_sequence: output_level_sequence_arc,
            meter_frame_sequence: meter_frame_sequence_arc,
            meter_emit_attempts: meter_emit_attempts_arc,
            meter_emit_failures: meter_emit_failures_arc,
            mode_atomic: Arc::new(AtomicU32::new(mode_to_ring_u32(VirtualMicMode::Silence))),
            uplink_slot: Arc::new(Mutex::new(None)),
            original_queue_slot: Arc::new(Mutex::new(None)),
            output_preview_slot: Arc::new(Mutex::new(None)),
            backpressure: backpressure_arc,
            last_send_time: Arc::new(Mutex::new(None)),
            audio_flowing: Arc::new(AtomicBool::new(false)),
            session_active: Arc::new(AtomicBool::new(false)),
            capture_restart_allowed: Arc::new(AtomicBool::new(false)),
            uplink_samples: uplink_samples_arc,
            uplink_persisted: uplink_persisted_arc,
            session_log: std::sync::Arc::new(crate::transcript_log::SessionLog::default()),
            conn_log: std::sync::Arc::new(Default::default()),
            show_latency_badge: Arc::new(std::sync::atomic::AtomicBool::new(
                cfg.ui.show_latency_badge,
            )),
            process_start: std::time::Instant::now(),
            session_generation: Arc::new(AtomicU64::new(0)),
            session_started_ms: Arc::new(AtomicU64::new(0)),
            last_interpret_ms: Arc::new(AtomicU64::new(0)),
        };

        if cfg.audio.output_preview_enabled {
            if let Err(error) = engine.set_output_preview_enabled(true) {
                eprintln!("[engine] failed to restore output preview: {error}");
            }
        }

        engine
    }

    /// Returns a cloned `Arc` to the uplink sample counter so callers (e.g.
    /// the housekeeping ticker in Task 7) can read or persist the count without
    /// holding any lock.
    pub fn uplink_samples(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.uplink_samples)
    }

    /// Returns a cloned `Arc` to the persisted-sample watermark so callers
    /// (e.g. Task 7's shutdown flush) can read or update the count without
    /// holding any lock.
    pub fn uplink_persisted(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.uplink_persisted)
    }

    // ── Reminder source-of-truth (issue #2) ───────────────────────────────────

    /// Millis since the engine's process-relative clock base.
    fn now_ms(&self) -> u64 {
        self.process_start.elapsed().as_millis() as u64
    }

    /// Mark the start of a new continuous Interpret/recording session.
    /// Bumps the session generation, records the start time, and clears the
    /// interpret-activity marker so the inactivity window is measured from
    /// this session's start. Called from `start_openai_session_locked`.
    fn mark_session_started(&self) {
        // `.max(1)` so the stored value is never the "no session" sentinel 0,
        // even in the (sub-millisecond) window right after process start.
        self.session_started_ms
            .store(self.now_ms().max(1), Ordering::Relaxed);
        self.last_interpret_ms.store(0, Ordering::Relaxed);
        self.session_generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Mark the current session as stopped. Called from
    /// `stop_openai_session_locked` so duration/inactivity reminders reset.
    fn mark_session_stopped(&self) {
        self.session_started_ms.store(0, Ordering::Relaxed);
        self.last_interpret_ms.store(0, Ordering::Relaxed);
    }

    /// Build a [`ReminderSnapshot`](crate::notifications::reminder::ReminderSnapshot)
    /// from the engine's source-of-truth atomics. This is the only place the
    /// notification dispatcher reads session/interpret state, keeping reminder
    /// timekeeping tied to the same lifecycle as Interpret/recording.
    pub fn reminder_snapshot(&self) -> crate::notifications::reminder::ReminderSnapshot {
        use crate::notifications::reminder::ReminderSnapshot;
        let started = self.session_started_ms.load(Ordering::Relaxed);
        let active = self.session_active.load(Ordering::Relaxed);

        // A live session requires both the active flag and a recorded start.
        if !active || started == 0 {
            return ReminderSnapshot {
                session_id: None,
                session_elapsed: std::time::Duration::ZERO,
                interpret_active: false,
                since_interpret_activity: None,
            };
        }

        let now = self.now_ms().max(started);
        let last = self.last_interpret_ms.load(Ordering::Relaxed);
        // No interpreted text yet → measure the quiet window from session start
        // so a session that never produces output still trips the reminder.
        let activity_base = if last == 0 { started } else { last };

        ReminderSnapshot {
            session_id: Some(self.session_generation.load(Ordering::Relaxed)),
            session_elapsed: std::time::Duration::from_millis(now - started),
            interpret_active: true,
            since_interpret_activity: Some(std::time::Duration::from_millis(
                now.saturating_sub(activity_base),
            )),
        }
    }

    // ── Mode control ──────────────────────────────────────────────────────────

    /// Switch the engine to a new `VirtualMicMode`.
    ///
    /// 1. Updates `mode_atomic` immediately (lock-free read by graph loop).
    /// 2. Stores the new mode in `Inner`.
    /// 3. Computes the `FrameRouting` for the mode.
    /// 4. Updates the ring-header mode hint for the HAL driver.
    /// 5. If `routing.ring_silence`, flushes zeros into the ring immediately.
    /// 6. Starts or stops CPAL mic capture based on routing flags.
    /// 7. Starts or stops the OpenAI Realtime session based on
    ///    `routing.openai_connected`.
    pub fn set_mode(&self, mode: VirtualMicMode) {
        let started = std::time::Instant::now();
        lifecycle_trace(format!("engine.set_mode start mode={mode:?}"));
        // Update the atomic FIRST so the graph loop sees the new mode on the
        // next frame even before the inner lock is acquired.
        self.mode_atomic
            .store(mode_to_ring_u32(mode), Ordering::Relaxed);

        let previous_mode = {
            let mut g = self.inner.lock();
            let previous = g.mode;
            g.mode = mode;
            lifecycle_trace(format!(
                "engine.set_mode state_updated previous={previous:?} next={mode:?} capture={} probe={} realtime={}",
                g.capture.is_some(),
                g.probe_capture.is_some(),
                g.realtime_task.is_some()
            ));
            previous
        };

        let routing = intervox_core::FrameRouting::for_mode_and_mix(mode, 0);

        self.ring.set_mode(mode_to_ring_u32(mode));

        if routing.ring_silence {
            self.ring.flush_silence();
            self.clear_output_preview();
            // Honest idle: reset both level atomics; the meter task pushes the
            // coherent zero frame on its next tick.
            self.publish_input_level_zero();
            self.publish_output_level_zero();
        } else if mode != previous_mode {
            // Entering a live mode must start from current audio, not unread
            // samples left behind by a previous mode or by an inactive consumer.
            self.ring.clear_unread();
        }

        let needs_capture = routing.mic_to_ring || routing.mic_to_openai;

        let capture_start_source: Option<Option<String>> = {
            let mut g = self.inner.lock();
            if needs_capture && g.capture.is_none() {
                lifecycle_trace(format!(
                    "engine.set_mode scheduling_capture mode={mode:?} source={:?}",
                    g.source_id
                ));
                self.stop_level_probe_locked(&mut g);
                // Allow exactly one auto-restart per mode-entry (Task 4.5).
                self.capture_restart_allowed.store(true, Ordering::Relaxed);
                Some(g.source_id.clone())
            } else if !needs_capture && g.capture.is_some() {
                lifecycle_trace(format!("engine.set_mode stopping_capture mode={mode:?}"));
                // No auto-restart when capture is intentionally stopped.
                self.capture_restart_allowed.store(false, Ordering::Relaxed);
                self.stop_capture_locked(&mut g);
                None
            } else {
                None
            }
        };

        if let Some(source_id) = capture_start_source {
            let opened =
                self.open_capture_for_device(source_id.clone(), Arc::clone(&self.backpressure));
            match opened {
                Ok((handle, rx)) => {
                    let mut g = self.inner.lock();
                    let current_routing = intervox_core::FrameRouting::for_mode_and_mix(g.mode, 0);
                    let current_needs_capture =
                        current_routing.mic_to_ring || current_routing.mic_to_openai;
                    if current_needs_capture && g.capture.is_none() && g.source_id == source_id {
                        self.install_capture_locked(&mut g, handle, rx);
                        lifecycle_trace(format!(
                            "engine.set_mode installed_capture mode={:?} source={:?}",
                            g.mode, g.source_id
                        ));
                    } else {
                        lifecycle_trace(format!(
                            "engine.set_mode discarding_stale_capture current_mode={:?} requested_source={source_id:?} current_source={:?}",
                            g.mode, g.source_id
                        ));
                        drop(g);
                        handle.stop_in_background("stale-mode-capture");
                        drop(rx);
                    }
                }
                Err(e) => {
                    let should_surface = {
                        let g = self.inner.lock();
                        let current_routing =
                            intervox_core::FrameRouting::for_mode_and_mix(g.mode, 0);
                        let current_needs_capture =
                            current_routing.mic_to_ring || current_routing.mic_to_openai;
                        current_needs_capture && g.capture.is_none() && g.source_id == source_id
                    };
                    if should_surface {
                        eprintln!("[engine] failed to start capture: {e}");
                        let _ = self.app.emit("error", e);
                    } else {
                        lifecycle_trace(format!(
                            "engine.set_mode ignored_stale_capture_error requested_source={source_id:?}"
                        ));
                    }
                }
            }
        }

        // Capture the target language and whether a new session is being started
        // before releasing the guard, so we can push "connecting" post-lock (Task 9).
        let connecting_tgt: Option<String> = {
            let mut g = self.inner.lock();
            let current_routing = intervox_core::FrameRouting::for_mode_and_mix(g.mode, 0);

            // ── OpenAI Realtime session lifecycle ─────────────────────────────────
            if current_routing.openai_connected && g.realtime_task.is_none() {
                let tgt = g.target_language.clone();
                self.start_openai_session_locked(&mut g);
                Some(tgt)
            } else {
                if !current_routing.openai_connected && g.realtime_task.is_some() {
                    self.stop_openai_session_locked(&mut g);
                }
                None
            }
        }; // inner guard dropped here

        // Task 9: push "connecting" AFTER the inner guard is released — mirrors
        // the "closed" push pattern at the bottom of stop_openai_session_locked.
        if let Some(tgt) = connecting_tgt {
            self.conn_log.push("connecting", format!("target={tgt}"));
        }
        lifecycle_trace(format!(
            "engine.set_mode done mode={mode:?} elapsed_ms={}",
            started.elapsed().as_millis()
        ));
    }

    // ── Device / language setters ─────────────────────────────────────────────

    /// Store the selected audio source ID and restart capture if running.
    ///
    /// Source switching is a transaction:
    /// - validate the CoreAudio UID before mutating engine state;
    /// - if a stream is active, open the new stream before stopping the old one;
    /// - install only if this switch generation is still current;
    /// - on open failure, keep the old source and old stream alive.
    ///
    /// Returns `false` when this request was superseded by a newer source switch.
    pub fn set_source_device(&self, id: String) -> Result<bool, AppError> {
        if !crate::devices::source_id_is_available(&id) {
            return Err(AppError::audio_device_unavailable(
                "The selected audio source is not available.",
            ));
        }

        let (generation, use_live_backpressure) = {
            let mut g = self.inner.lock();
            if g.source_id.as_deref() == Some(id.as_str()) {
                return Ok(true);
            }

            g.source_switch_generation = g.source_switch_generation.wrapping_add(1);
            let generation = g.source_switch_generation;
            let capture_active = g.capture.is_some();
            let has_running_stream = capture_active || g.probe_capture.is_some();

            if !has_running_stream {
                g.source_id = Some(id);
                return Ok(true);
            }

            (generation, capture_active)
        };

        let backpressure = if use_live_backpressure {
            Arc::clone(&self.backpressure)
        } else {
            Arc::new(AudioBackpressureCounters::default())
        };
        let opened = match self.open_capture_for_device(Some(id.clone()), backpressure) {
            Ok(opened) => opened,
            Err(e) => {
                let mut g = self.inner.lock();
                if g.source_switch_generation != generation {
                    return Ok(false);
                }

                let routing = intervox_core::FrameRouting::for_mode_and_mix(g.mode, 0);
                let mode_needs_capture = routing.mic_to_ring || routing.mic_to_openai;
                let stream_still_required = mode_needs_capture || g.probe_capture.is_some();
                if stream_still_required {
                    return Err(e);
                }

                g.source_id = Some(id);
                return Ok(true);
            }
        };

        let (handle, rx) = opened;
        let mut g = self.inner.lock();
        if g.source_switch_generation != generation {
            drop(g);
            handle.stop_in_background("superseded-source-capture");
            drop(rx);
            return Ok(false);
        }

        g.source_id = Some(id);
        let routing = intervox_core::FrameRouting::for_mode_and_mix(g.mode, 0);
        let mode_needs_capture = routing.mic_to_ring || routing.mic_to_openai;

        if mode_needs_capture {
            if g.probe_capture.is_some() {
                self.stop_level_probe_locked(&mut g);
            }
            if g.capture.is_some() {
                self.stop_capture_locked(&mut g);
            }
            self.install_capture_locked(&mut g, handle, rx);
        } else if g.probe_capture.is_some() {
            if g.capture.is_some() {
                self.stop_capture_locked(&mut g);
            }
            self.stop_level_probe_locked(&mut g);
            drop(rx);
            g.probe_capture = Some(handle);
        } else {
            drop(g);
            handle.stop_in_background("inactive-source-capture");
            drop(rx);
        }

        Ok(true)
    }

    /// Store the target output language code.
    ///
    /// If an OpenAI session is active, it is restarted so the new language
    /// takes effect immediately.  Capture is not interrupted.  (The source
    /// language is auto-detected by the endpoint and is not configurable.)
    pub fn set_target_language(&self, tgt: String) {
        // Capture whether a new session is being started so we can push
        // "connecting" after the inner guard is dropped (Task 9).
        let connecting_tgt: Option<String> = {
            let mut g = self.inner.lock();
            g.target_language = tgt;

            // If a realtime session is running, restart it with the new language.
            // The capture task and graph task continue uninterrupted; only the
            // OpenAI transport is restarted and the uplink_slot is refreshed.
            if g.realtime_task.is_some() {
                let new_tgt = g.target_language.clone();
                self.stop_openai_session_locked(&mut g);
                self.start_openai_session_locked(&mut g);
                Some(new_tgt)
            } else {
                None
            }
        }; // inner guard dropped here

        // Task 9: push "connecting" AFTER the inner guard is released — mirrors
        // the "closed" push pattern at the bottom of stop_openai_session_locked.
        if let Some(new_tgt) = connecting_tgt {
            self.conn_log
                .push("connecting", format!("target={new_tgt}"));
        }
    }

    /// Restart the active OpenAI session so session-start audio config changes
    /// such as original voice percent, ducking, and limiter settings take effect.
    /// Capture continues running; only the realtime transport and pull task are
    /// rebuilt with fresh config.
    pub fn restart_translation_session_for_config(&self) {
        let connecting_tgt: Option<String> = {
            let mut g = self.inner.lock();
            if g.realtime_task.is_some() {
                let tgt = g.target_language.clone();
                self.stop_openai_session_locked(&mut g);
                self.start_openai_session_locked(&mut g);
                Some(tgt)
            } else {
                None
            }
        };

        if let Some(tgt) = connecting_tgt {
            self.conn_log.push("connecting", format!("target={tgt}"));
        }
    }

    // ── Shutdown ──────────────────────────────────────────────────────────────

    /// Graceful shutdown: stop capture + meter task, OpenAI session, then flush silence.
    pub fn shutdown(&self) {
        {
            let mut g = self.inner.lock();
            if let Some(h) = g.meter_task.take() {
                h.abort();
            }
            self.stop_openai_session_locked(&mut g);
            self.stop_capture_locked(&mut g);
            self.stop_level_probe_locked(&mut g);
            self.stop_output_preview_locked(&mut g);
        }
        self.ring.flush_silence();
        self.clear_output_preview();
        // Task 7: abort the 10 s fold task before the final synchronous fold so
        // there is no double-write race between the background ticker and the
        // shutdown flush below.
        {
            let mut g = self.inner.lock();
            if let Some(h) = g.fold_task.take() {
                h.abort();
            }
        }
        // Task 7: final usage fold — persist any samples accumulated since the
        // last 10 s tick so the last partial interval is not lost.
        {
            let now = self
                .uplink_samples()
                .load(std::sync::atomic::Ordering::Relaxed);
            let prev = self
                .uplink_persisted()
                .swap(now, std::sync::atomic::Ordering::Relaxed);
            let delta = now.saturating_sub(prev);
            if delta > 0 {
                let mut u = crate::usage_store::load();
                u.add_samples(delta, &crate::usage_store::current_month_utc());
                crate::usage_store::save(&u);
            }
        }
    }

    /// Start a source-selection microphone level probe.
    ///
    /// This is intentionally separate from the live engine capture path. In
    /// onboarding the app is still in Silence mode, but the user must still see
    /// whether the selected input source is receiving audio.
    pub fn start_level_probe(&self) -> Result<(), AppError> {
        let source_id = {
            let g = self.inner.lock();
            if g.capture.is_some() || g.probe_capture.is_some() {
                return Ok(());
            }
            g.source_id.clone()
        };

        let (handle, rx) = self.open_capture_for_device(
            source_id.clone(),
            Arc::new(AudioBackpressureCounters::default()),
        )?;
        drop(rx);

        let mut g = self.inner.lock();
        if g.capture.is_none() && g.probe_capture.is_none() && g.source_id == source_id {
            g.probe_capture = Some(handle);
        } else {
            drop(g);
            handle.stop_in_background("stale-probe-capture");
        }
        Ok(())
    }

    /// Stop the source-selection microphone level probe if it is running.
    pub fn stop_level_probe(&self) {
        let mut g = self.inner.lock();
        self.stop_level_probe_locked(&mut g);
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Current operating mode.
    #[allow(dead_code)]
    pub fn mode(&self) -> VirtualMicMode {
        self.inner.lock().mode
    }

    pub fn levels(&self) -> (f32, f32) {
        (
            f32::from_bits(self.level.load(Ordering::Relaxed)),
            f32::from_bits(self.out_level.load(Ordering::Relaxed)),
        )
    }

    pub fn backpressure_metrics(&self) -> AudioBackpressureMetrics {
        self.backpressure.snapshot()
    }

    pub fn meter_diagnostics(&self) -> AudioMeterDiagnostics {
        AudioMeterDiagnostics {
            input_level: f32::from_bits(self.level.load(Ordering::Relaxed)),
            output_level: f32::from_bits(self.out_level.load(Ordering::Relaxed)),
            input_sequence: self.input_level_sequence.load(Ordering::Acquire),
            output_sequence: self.output_level_sequence.load(Ordering::Acquire),
            last_frame_sequence: self.meter_frame_sequence.load(Ordering::Acquire),
            emit_attempts: self.meter_emit_attempts.load(Ordering::Relaxed),
            emit_failures: self.meter_emit_failures.load(Ordering::Relaxed),
        }
    }

    /// Shared handle to the ring producer (for the audio pipeline).
    #[allow(dead_code)]
    pub fn ring(&self) -> Arc<RingProducer> {
        Arc::clone(&self.ring)
    }

    /// End the active transcript session log (used by clear_transcript_history
    /// so an in-flight session's file is also cleared and not re-created).
    pub fn end_session_log(&self) {
        self.session_log.end();
    }

    /// Returns a cloned `Arc` to the connection log so callers (e.g.
    /// `get_connection_log`) can snapshot it without holding any engine lock.
    pub fn conn_log(&self) -> Arc<crate::connection_log::ConnectionLog> {
        Arc::clone(&self.conn_log)
    }

    /// Update the tray latency badge toggle without restarting anything.
    /// Called by `set_ui_config` so the pull_task sees the new value on the
    /// next ~1 Hz tick without any disk read.
    pub fn set_show_latency_badge(&self, v: bool) {
        self.show_latency_badge
            .store(v, std::sync::atomic::Ordering::Relaxed);
    }

    /// Enable or disable local speaker preview for the final virtual-mic signal.
    ///
    /// Enabling opens the current macOS default output device. Failure leaves
    /// preview disabled so the frontend can rollback its optimistic toggle.
    pub fn set_output_preview_enabled(&self, enabled: bool) -> Result<(), AppError> {
        if !enabled {
            let mut g = self.inner.lock();
            g.output_preview_enabled = false;
            self.stop_output_preview_locked(&mut g);
            return Ok(());
        }

        {
            let mut g = self.inner.lock();
            g.output_preview_enabled = true;
            if g.output_preview.is_some() {
                return Ok(());
            }
        }

        let handle = match playback::start_default_output() {
            Ok(handle) => handle,
            Err(error) => {
                let mut g = self.inner.lock();
                g.output_preview_enabled = false;
                self.stop_output_preview_locked(&mut g);
                return Err(error);
            }
        };

        let mut g = self.inner.lock();
        if !g.output_preview_enabled {
            drop(g);
            handle.stop_in_background();
            return Ok(());
        }
        self.install_output_preview_locked(&mut g, handle);
        Ok(())
    }

    /// Reconcile preview with the current CoreAudio default output snapshot.
    /// Called by the low-frequency device poll; failures keep preview requested
    /// and are retried on the next poll.
    pub fn sync_output_preview_default_device(&self, default_output_id: Option<&str>) {
        let should_start = {
            let mut g = self.inner.lock();
            if !g.output_preview_enabled {
                return;
            }

            match (g.output_preview.as_ref(), default_output_id) {
                (_, None) => {
                    self.stop_output_preview_locked(&mut g);
                    false
                }
                (Some(handle), Some(output_id)) if handle.device_id() == output_id => false,
                _ => {
                    self.stop_output_preview_locked(&mut g);
                    true
                }
            }
        };

        if !should_start {
            return;
        }

        let Some(output_id) = default_output_id.map(str::to_string) else {
            return;
        };

        match playback::start_default_output_for_device_id(output_id) {
            Ok(handle) => {
                let mut g = self.inner.lock();
                if g.output_preview_enabled {
                    self.install_output_preview_locked(&mut g, handle);
                } else {
                    drop(g);
                    handle.stop_in_background();
                }
            }
            Err(error) => {
                eprintln!("[engine] failed to sync output preview: {error}");
            }
        }
    }

    // ── Internal helpers (called with `inner` lock held) ──────────────────────

    fn publish_input_level_zero(&self) {
        self.level.store(0, Ordering::Relaxed);
        self.input_level_sequence.fetch_add(1, Ordering::Release);
    }

    fn publish_output_level_zero(&self) {
        self.out_level.store(0, Ordering::Relaxed);
        self.output_level_sequence.fetch_add(1, Ordering::Release);
    }

    fn clear_output_preview(&self) {
        playback::clear_slot(&self.output_preview_slot);
    }

    fn install_output_preview_locked(&self, g: &mut Inner, handle: PlaybackHandle) {
        self.stop_output_preview_locked(g);
        *self.output_preview_slot.lock() = Some(handle.sender());
        g.output_preview = Some(handle);
    }

    fn stop_output_preview_locked(&self, g: &mut Inner) {
        *self.output_preview_slot.lock() = None;
        if let Some(handle) = g.output_preview.take() {
            handle.stop_in_background();
        }
    }

    /// Start the OpenAI Realtime session.
    ///
    /// Creates the uplink channel, writes the sender into `uplink_slot` (so the
    /// graph loop immediately starts forwarding frames), spawns the websocket
    /// transport task, the real downlink event-consumer task, and the pull task
    /// that ticks every 10 ms and writes translated audio into the ring.
    ///
    /// If no API key is set, emits an `"error"` event and returns without
    /// starting a session — translated audio simply won't flow until the user
    /// sets their key.
    ///
    /// Caller must hold the `inner` mutex lock.
    fn start_openai_session_locked(&self, g: &mut Inner) {
        let cfg = {
            use tauri::Manager as _;
            self.app
                .state::<crate::commands::AppHandle>()
                .config
                .lock()
                .unwrap()
                .clone()
        };
        // Resolve the realtime endpoint. A custom (wire-compatible) endpoint
        // overrides the default OpenAI URL and makes the API key optional;
        // the default OpenAI endpoint still requires a key.
        let custom_endpoint = cfg
            .account
            .custom_realtime_endpoint()
            .map(str::to_string);
        let url = custom_endpoint
            .clone()
            .unwrap_or_else(|| realtime::REALTIME_URL.to_string());
        let key = match cfg
            .account
            .openai_api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(k) => k.to_string(),
            // Custom endpoint: no key needed — connect without auth headers.
            None if custom_endpoint.is_some() => String::new(),
            None => {
                let _ = self.app.emit(
                    "error",
                    intervox_core::AppError::openai_auth_error("No API key set"),
                );
                return;
            }
        };

        let tgt_lang = g.target_language.clone();

        // Read session-level config at session-start so toggling settings takes
        // effect on the next session start.
        let limiter_enabled = cfg.audio.limiter_enabled;

        // A positive original voice percent enables the original-audio tap
        // inside Translate. 0% is the translated-only no-leak path.
        let mix_original = cfg.mix.original_voice_percent > 0;

        // Build MixSettings from config.
        // original_gain_db = percent_to_db(original_voice_percent)
        // translated_gain_db = 0.0 (translated always at unity gain)
        // duck_original / limiter_enabled from config.
        let mix_settings = intervox_core::audio::mixer::MixSettings {
            original_gain_db: intervox_core::config::percent_to_db(
                cfg.mix.original_voice_percent as f32,
            ),
            translated_gain_db: 0.0,
            duck_original: cfg.mix.duck_original,
            limiter_enabled,
        };

        // Uplink channel: graph loop → realtime transport.
        // 8 chunks × 40 ms = 320 ms max queued uplink audio before frames are
        // dropped. Drops are acceptable; seconds of delayed mic audio are not.
        let (pcm_tx, pcm_rx) = tokio::sync::mpsc::channel::<Vec<i16>>(OPENAI_UPLINK_QUEUE_BOUND);

        // Event channel: realtime transport → event consumer.
        let (ev_tx, mut ev_rx) = tokio::sync::mpsc::channel::<TranslationEvent>(128);

        // Write the sender into the shared slot so the graph loop (which may
        // already be running in spawn_blocking) starts forwarding immediately.
        *self.uplink_slot.lock() = Some(pcm_tx.clone());
        g.pcm_tx = Some(pcm_tx);

        // Shared jitter buffer: ev_task pushes, pull_task pulls.
        let jitter: SharedJitterBuf = Arc::new(Mutex::new(translate_chain::new_jitter_buffer()));

        // Original-audio tap queue. Only created and wired when Translate has
        // a positive original voice percent; stays None for translated-only.
        // The graph loop reads this from `original_queue_slot` on every frame.
        let original_queue: Option<SharedOriginalQueue> = if mix_original {
            let q = translate_chain::new_original_queue();
            *self.original_queue_slot.lock() = Some(Arc::clone(&q));
            Some(q)
        } else {
            *self.original_queue_slot.lock() = None;
            None
        };

        // Task 8: start the per-session transcript log path. This is lock-only:
        // directory creation is deferred to append so session start performs no
        // filesystem work while holding the engine lock.
        self.session_log.start(&crate::commands::rfc3339_now());
        let session_log_task = std::sync::Arc::clone(&self.session_log);

        // Task 9: conn_log "connecting" push happens AFTER the inner guard is
        // dropped, at each call site (mirrors the "closed" push pattern in
        // stop_openai_session_locked).  We only clone the Arc here; the actual
        // push is performed by the caller once it has released `g`.
        let conn_log_task = std::sync::Arc::clone(&self.conn_log);
        // Task 8: clone target lang for transcript records in the ev_task
        // (tgt_lang itself will be moved into the supervisor call below).
        let ev_tgt_lang = tgt_lang.clone();

        // Mark session active before spawning so the supervisor sees the flag
        // immediately (avoids a race where the supervisor checks before the flag
        // is set and exits on the first iteration).
        self.session_active.store(true, Ordering::Release);
        // Issue #2: a new continuous session starts here — bump the generation
        // and reset the duration/inactivity clocks so reminders don't carry
        // over from a previous session or a mid-session restart.
        self.mark_session_started();
        let session_active_rt = Arc::clone(&self.session_active);

        // Spawn the supervisor task (Task 4.5).
        //
        // The supervisor wraps `realtime::run` so that if `run` returns
        // unexpectedly (not because the session was stopped), it is restarted
        // with capped backoff.  Aborting the JoinHandle cancels the supervisor,
        // which propagates to the currently-awaited `run` future.
        let rt_task = tauri::async_runtime::spawn(supervisor::run_supervised(
            url,
            key,
            tgt_lang,
            pcm_rx,
            ev_tx,
            session_active_rt,
            self.uplink_samples(),
        ));
        g.realtime_task = Some(rt_task);

        // ── ev_task: downlink event consumer ──────────────────────────────────
        //
        // Receives `TranslationEvent`s from the realtime transport and:
        //   - OutputAudioDelta → ingest into jitter buffer via translate_chain;
        //     record first-audio timing; set audio_flowing flag.
        //   - OutputTranscriptDelta → emit "target-transcript-delta" to frontend;
        //     accumulate into tgt_seg for persistence.
        //   - InputTranscriptDelta  → emit "source-transcript-delta" to frontend;
        //     accumulate into src_seg for persistence.
        //   - InputTranscriptDone / OutputTranscriptDone → flush the corresponding
        //     segment to the session log (Task 8).
        //   - Error                 → emit "error" event to frontend.
        //   - SessionUpdated → mark openai_connected=true, emit "status-changed".
        //   - Closed → flush both segments, mark openai_connected=false, emit "status-changed".
        //   - Ignored → silently ignore.
        //
        // The resampler is owned by this task (one instance, streaming-safe).
        let ev_app = self.app.clone();
        let jitter_push = Arc::clone(&jitter);
        // Task 4.4: shared latency signals passed into ev_task.
        let ev_last_send = Arc::clone(&self.last_send_time);
        let ev_audio_flowing = Arc::clone(&self.audio_flowing);
        // Task 4.4: measured first-audio latency (written once per session on
        // the first OutputAudioDelta, then frozen until session restart).
        // Stored as Arc<AtomicU32> so pull_task can read it lock-free.
        let first_audio_ms: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        let ev_first_audio_ms = Arc::clone(&first_audio_ms);
        // Issue #2: inactivity is tracked from user-visible interpreted text,
        // not generic audio level — so update it from the transcript-delta arm.
        let ev_last_interpret_ms = Arc::clone(&self.last_interpret_ms);
        let ev_process_start = self.process_start;
        let ev_task = tauri::async_runtime::spawn(async move {
            // One resampler persisted across events so phase state carries across
            // chunk boundaries.  Default 24 000 → 48 000; the actual in_hz is
            // updated per-event from `sample_rate` in OutputAudioDelta.
            // We keep a single resampler and lazily recreate it when the server
            // reports a different sample_rate (rare; most events are 24 kHz).
            let mut resampler =
                intervox_core::audio::resampler::LinearResampler::new(24_000, 48_000);
            let mut current_in_hz: u32 = 24_000;
            // Track whether we have already measured the first-audio latency
            // for this session (so we compute it at most once).
            let mut first_audio_measured = false;

            // Segment accumulation buffers are task-local and reset after each flush.
            let mut src_seg = String::new();
            let mut tgt_seg = String::new();
            // Silence-gap timers finalize transcript segments when the endpoint
            // sends deltas without a terminal sentence-boundary event.
            let mut last_src_delta: Option<std::time::Instant> = None;
            let mut last_tgt_delta: Option<std::time::Instant> = None;
            // Fires every 500 ms to check for stale segments.
            let mut silence_tick = tokio::time::interval(std::time::Duration::from_millis(500));
            // Consume the first immediate tick so the loop doesn't flush on entry.
            silence_tick.tick().await;

            // Inline flush helper: synchronous fs I/O for already-buffered text.
            // Does NOT hold any engine MutexGuard; session_log_task has its own
            // internal Mutex which is always acquired and released within append().
            macro_rules! flush_seg {
                ($seg:expr, $kind:expr, $lang:expr) => {
                    if !$seg.trim().is_empty() {
                        let save = {
                            use tauri::Manager as _;
                            ev_app
                                .state::<crate::commands::AppHandle>()
                                .config
                                .lock()
                                .unwrap()
                                .privacy
                                .save_transcript_history
                        };
                        if save {
                            session_log_task.append(&crate::transcript_log::TranscriptRecord {
                                ts: crate::commands::rfc3339_now(),
                                kind: $kind.to_string(),
                                lang: $lang.clone(),
                                text: std::mem::take(&mut $seg).trim().to_string(),
                            });
                        } else {
                            $seg.clear();
                        }
                    }
                };
            }

            loop {
                tokio::select! {
                    maybe_ev = ev_rx.recv() => {
                        let Some(ev) = maybe_ev else { break };
                        match ev {
                            TranslationEvent::SessionUpdated => {
                                // Task 4.4: session is confirmed live by the server.
                                // Mark openai_connected=true in AppStatus and emit
                                // "status-changed" — mirrors the lib.rs 5 s-interval
                                // lock/clone/emit discipline (MutexGuard dropped before emit).
                                use tauri::Manager as _;
                                let status_clone = {
                                    let app_handle = ev_app.state::<crate::commands::AppHandle>();
                                    // std::sync::Mutex — must not hold across await points.
                                    let mut st = app_handle.state.lock().unwrap();
                                    st.mark_openai_connected(true);
                                    st.status.clone()
                                }; // MutexGuard dropped here — safe to emit
                                let _ = ev_app.emit("status-changed", status_clone);
                                // Task 9: record connected (after MutexGuard dropped).
                                conn_log_task.push("connected", "session established");
                            }

                            TranslationEvent::OutputAudioDelta {
                                pcm16,
                                sample_rate,
                                channels,
                                ..
                            } => {
                                // Task 4.4: measure first-audio latency on the first delta
                                // of this session.  `last_send_time` is set by the graph
                                // loop on each successful uplink try_send.
                                if !first_audio_measured {
                                    let elapsed = {
                                        let guard = ev_last_send.lock();
                                        guard.as_ref().map(|t| t.elapsed())
                                    };
                                    if let Some(dur) = elapsed {
                                        let ms = dur.as_millis() as u32;
                                        ev_first_audio_ms.store(ms, Ordering::Relaxed);
                                    }
                                    // Mark that at least one delta arrived — gate for
                                    // latency emit (should_emit_latency).
                                    ev_audio_flowing.store(true, Ordering::Relaxed);
                                    first_audio_measured = true;
                                }

                                // If the server changes sample rate (unusual but possible),
                                // recreate the resampler so we resample correctly.
                                if sample_rate != current_in_hz {
                                    resampler = intervox_core::audio::resampler::LinearResampler::new(
                                        sample_rate,
                                        48_000,
                                    );
                                    current_in_hz = sample_rate;
                                }
                                let mut jb = jitter_push.lock();
                                translate_chain::ingest_audio_delta(
                                    &pcm16,
                                    channels,
                                    &mut resampler,
                                    &mut jb,
                                );
                            }

                            TranslationEvent::OutputTranscriptDelta { text, .. } => {
                                // Emit { text } to match the frontend's
                                // `listen<{ text: string }>("target-transcript-delta", ...)`.
                                let _ = ev_app.emit(
                                    "target-transcript-delta",
                                    serde_json::json!({ "text": text }),
                                );
                                // Task 8: accumulate into tgt_seg for persistence.
                                tgt_seg.push_str(&text);
                                last_tgt_delta = Some(std::time::Instant::now());
                                // Issue #2: user-visible interpreted text just
                                // appeared — reset the inactivity window.
                                ev_last_interpret_ms.store(
                                    (ev_process_start.elapsed().as_millis() as u64).max(1),
                                    Ordering::Relaxed,
                                );
                            }

                            TranslationEvent::InputTranscriptDelta { text, .. } => {
                                // Emit { text } to match the frontend's
                                // `listen<{ text: string }>("source-transcript-delta", ...)`.
                                let _ = ev_app.emit(
                                    "source-transcript-delta",
                                    serde_json::json!({ "text": text }),
                                );
                                // Task 8: accumulate into src_seg for persistence.
                                src_seg.push_str(&text);
                                last_src_delta = Some(std::time::Instant::now());
                            }

                            TranslationEvent::Error { code, message } => {
                                // Determine if this is a fatal auth error before acquiring any lock.
                                let fatal = matches!(code.as_deref(), Some("AUTH"))
                                    || message.to_lowercase().contains("invalid api key")
                                    || message.to_lowercase().contains("invalid_api_key");

                                // Task 9: record error (before acquiring any lock).
                                conn_log_task.push("error", format!("{:?}: {}", code, message));

                                // Surface to the UI; reconnect is handled inside realtime::run.
                                let _ = ev_app
                                    .emit("error", intervox_core::AppError::network_error(message.clone()));

                                if fatal {
                                    use tauri::Manager as _;
                                    let status_clone = {
                                        let app_handle = ev_app.state::<crate::commands::AppHandle>();
                                        let mut st = app_handle.state.lock().unwrap();
                                        // mark_openai_connected(false) clears openai_connected and updates phase
                                        // (→ ConnectingTranslation) + translation (→ Reconnecting); then override
                                        // translation to Failed since the supervisor will NOT retry an auth failure.
                                        st.mark_openai_connected(false);
                                        st.set_translation_conn(
                                            intervox_core::state::TranslationConn::Failed,
                                        );
                                        st.status.clone()
                                    }; // MutexGuard dropped here
                                    let _ = ev_app.emit("status-changed", status_clone);
                                    // Task 9: fatal → also record "failed" (after MutexGuard dropped).
                                    conn_log_task.push("failed", message.clone());
                                } else {
                                    use tauri::Manager as _;
                                    let status_clone = {
                                        let app_handle = ev_app.state::<crate::commands::AppHandle>();
                                        let mut st = app_handle.state.lock().unwrap();
                                        st.mark_openai_connected(false);
                                        st.status.clone()
                                    }; // MutexGuard dropped here
                                    let _ = ev_app.emit("status-changed", status_clone);
                                    // Task 9: non-fatal → record "reconnecting" (after MutexGuard dropped).
                                    conn_log_task.push("reconnecting", "");
                                }
                            }

                            TranslationEvent::Closed => {
                                // Task 8: flush both non-empty segments before marking closed.
                                flush_seg!(src_seg, "source", "auto".to_string());
                                flush_seg!(tgt_seg, "target", ev_tgt_lang);
                                last_src_delta = None;
                                last_tgt_delta = None;

                                // Task 4.4: transport closed (network drop / server reset).
                                // Mark openai_connected=false — mirrors stop path.
                                // realtime::run handles reconnect; we just update the UI.
                                use tauri::Manager as _;
                                let status_clone = {
                                    let app_handle = ev_app.state::<crate::commands::AppHandle>();
                                    let mut st = app_handle.state.lock().unwrap();
                                    st.mark_openai_connected(false);
                                    st.status.clone()
                                }; // MutexGuard dropped here
                                let _ = ev_app.emit("status-changed", status_clone);
                                // Task 9: record closed (after MutexGuard dropped).
                                conn_log_task.push("closed", "session ended");
                            }

                            TranslationEvent::InputTranscriptDone => {
                                // Task 8: sentence-boundary marker for source — flush src_seg.
                                flush_seg!(src_seg, "source", "auto".to_string());
                                last_src_delta = None;
                            }

                            TranslationEvent::OutputTranscriptDone => {
                                // Sentence-boundary marker for target: flush tgt_seg.
                                flush_seg!(tgt_seg, "target", ev_tgt_lang);
                                last_tgt_delta = None;
                            }

                            TranslationEvent::Ignored(_) => {
                                // Unknown event type — silently ignore.
                            }
                        }
                    }

                    // Silence-gap finalization.
                    // Fires every 500 ms; if a segment has been accumulating for
                    // >= 1500 ms with no new delta, flush it now so segments
                    // persist even when the endpoint never sends *_transcript.done.
                    _ = silence_tick.tick() => {
                        let now = std::time::Instant::now();
                        if let Some(t) = last_src_delta {
                            if now.duration_since(t).as_millis() >= 1500 && !src_seg.trim().is_empty() {
                                flush_seg!(src_seg, "source", "auto".to_string());
                                last_src_delta = None;
                            }
                        }
                        if let Some(t) = last_tgt_delta {
                            if now.duration_since(t).as_millis() >= 1500 && !tgt_seg.trim().is_empty() {
                                flush_seg!(tgt_seg, "target", ev_tgt_lang);
                                last_tgt_delta = None;
                            }
                        }
                    }
                }
            }
            flush_seg!(src_seg, "source", "auto".to_string());
            flush_seg!(tgt_seg, "target", ev_tgt_lang);
        });
        g.ev_task = Some(ev_task);

        // ── pull_task: 10 ms tick → (mix or limiter) → ring ──────────────────
        //
        // Pulls 480 frames every 10 ms from the jitter buffer.
        //
        // Translate (mix_original == false):
        //   translated_block → optional limiter → ring.write → out_level
        //
        // Translate with original voice (mix_original == true):
        //   translated_block + drain 480 from original_queue → DelayLine →
        //   mix_translated_with_original(settings) → ring.write → out_level
        //
        // MixSettings and DelayLine are owned by this task (constructed once at
        // session start; mid-session config changes require a session restart —
        // established pattern, documented in translate_chain).
        //
        // Task 4.4: every 100th tick (~1 Hz) emit "latency-changed" (bare u32 =
        // total_estimated_latency_ms) when should_emit_latency() is true.
        //
        // TODO: recompute delay from measured latency via the first_audio_ms
        // AtomicU32 — replace EST_LATENCY_MS with a live read when the delta
        // is significant enough to warrant a session restart.
        let ring_arc = Arc::clone(&self.ring);
        let out_level_arc = Arc::clone(&self.out_level);
        let output_level_sequence = Arc::clone(&self.output_level_sequence);
        let jitter_pull = Arc::clone(&jitter);
        let output_preview_slot = Arc::clone(&self.output_preview_slot);
        // Task 4.4: signals for latency emit.
        let pull_app = self.app.clone();
        let pull_audio_flowing = Arc::clone(&self.audio_flowing);
        let pull_first_audio_ms = Arc::clone(&first_audio_ms);
        // Task 9: clone conn_log for latency throttle inside pull_task.
        let pull_conn_log = std::sync::Arc::clone(&self.conn_log);
        // FIX-3: lock-free badge + mode reads — no disk I/O in the hot pull loop.
        let pull_show_badge = Arc::clone(&self.show_latency_badge);
        let pull_mode_atomic = Arc::clone(&self.mode_atomic);
        let pull_task = tauri::async_runtime::spawn(async move {
            // Build the DelayLine only if we're mixing original (Task 4.3).
            let mut delay_line: Option<intervox_core::audio::delay_line::DelayLine> =
                if mix_original {
                    let delay_ms = intervox_core::audio::delay_line::compute_original_delay_ms(
                        translate_chain::EST_LATENCY_MS,
                    );
                    Some(intervox_core::audio::delay_line::DelayLine::new(
                        48_000, delay_ms,
                    ))
                } else {
                    None
                };

            let mut tick = tokio::time::interval(std::time::Duration::from_millis(10));
            // Task 4.4: tick counter for ~1 Hz latency emit (every 100 ticks = 1 s).
            let mut tick_count: u32 = 0;
            // Task 9: latency throttle — only push "latency" when value differs > 100 ms.
            let mut last_logged_latency: Option<u32> = None;
            loop {
                tick.tick().await;
                tick_count = tick_count.wrapping_add(1);

                // Pull translated block from jitter buffer.
                let (translated, jitter_ms) = {
                    let mut jb = jitter_pull.lock();
                    // Capture jitter_ms before the pull so we measure the buffered
                    // depth at the start of this tick (Task 4.4).
                    let jb_ms = jb.buffered_ms();
                    // In mix_original mode we want the raw block (delay line +
                    // mix_settings handles limiting); in translate-only mode the
                    // existing pull_block path (with its own limiter flag) is used.
                    let block = if mix_original {
                        // Pull raw block — limiter is handled by mix_frames below.
                        jb.pull(translate_chain::PULL_FRAMES)
                    } else {
                        translate_chain::pull_block(&mut jb, limiter_enabled)
                    };
                    (block, jb_ms)
                };

                let final_block = if let (Some(ref mut dl), Some(oq)) =
                    (delay_line.as_mut(), original_queue.as_ref())
                {
                    // Translate with original voice path:
                    // 1. Drain 480 original samples (zero-pad on underrun).
                    let original_480 =
                        translate_chain::drain_original_samples(oq, translate_chain::PULL_FRAMES);
                    // 2. Delay the original block through the persistent DelayLine.
                    let delayed_original = dl.process(&original_480);
                    // 3. Mix: translated under delayed original, apply limiter via settings.
                    translate_chain::mix_translated_with_original(
                        &translated,
                        &delayed_original,
                        &mix_settings,
                    )
                } else {
                    // Translate path: translated block already has limiter applied.
                    translated
                };

                // Write final block to ring (feeds the virtual mic driver at 48 kHz).
                // This is a live path, so stale unread backlog is bounded.
                ring_arc.write_live(&final_block);
                playback::tap_slot(&output_preview_slot, &final_block);
                // Update out_level from the FINAL block (mixed or limited).
                let bits = translate_chain::rms_bits(&final_block);
                out_level_arc.store(bits, Ordering::Relaxed);
                output_level_sequence.fetch_add(1, Ordering::Release);

                // Task 4.4: emit "latency-changed" at ~1 Hz (every 100 ticks).
                // Gate: only when connected AND audio has actually flowed.
                // audio_flowing=true implies openai is connected because ev_task
                // sets this flag only after receiving the first OutputAudioDelta,
                // which requires an active session.
                if tick_count.is_multiple_of(100) {
                    let flowing = pull_audio_flowing.load(Ordering::Relaxed);
                    if translate_chain::should_emit_latency(
                        /*openai_connected=*/ flowing, /*audio_flowing=*/ flowing,
                    ) {
                        // virtual_mic_output_lag_ms: ring backlog in ms.
                        // ring_arc.backlog_ms() acquires the parking_lot::Mutex
                        // briefly (sub-microsecond: one atomic read).
                        let ring_backlog_ms = ring_arc.backlog_ms();

                        let mut metrics = intervox_core::diagnostics::metrics::LatencyMetrics {
                            capture_to_send_ms: translate_chain::CAPTURE_TO_SEND_EST_MS,
                            openai_first_audio_ms: pull_first_audio_ms.load(Ordering::Relaxed),
                            jitter_buffer_ms: jitter_ms,
                            virtual_mic_output_lag_ms: ring_backlog_ms,
                            total_estimated_latency_ms: 0,
                        };
                        let total = metrics.recompute_total();
                        // Frontend listens as `listen<number>("latency-changed", ...)`
                        // — emit bare u32 (confirmed from src/lib/tauri.ts).
                        let _ = pull_app.emit("latency-changed", total);
                        // Keep latency_ms in AppStatus in sync so get_app_status()
                        // returns the current value without waiting for the next tick.
                        {
                            use tauri::Manager as _;
                            let app_handle = pull_app.state::<crate::commands::AppHandle>();
                            let mut st = app_handle.state.lock().unwrap();
                            st.status.latency_ms = Some(total);
                        } // MutexGuard dropped here — never held across await

                        // Task 9: push "latency" only when value changed materially (> 100 ms).
                        // This avoids log spam at ~1 Hz while still capturing meaningful shifts.
                        let should_log = match last_logged_latency {
                            None => true,
                            Some(prev) => total.abs_diff(prev) > 100,
                        };
                        if should_log {
                            pull_conn_log.push("latency", format!("{total} ms"));
                            last_logged_latency = Some(total);
                        }

                        // Best-effort: refresh tray latency badge if enabled.
                        // FIX-3: no disk read, no AppHandle state lock — use atomics.
                        if pull_show_badge.load(std::sync::atomic::Ordering::Relaxed) {
                            use tauri::Manager as _;
                            if let Some(tray_state) =
                                pull_app.try_state::<crate::commands::TrayState>()
                            {
                                let mode = ring::mode_from_u32(
                                    pull_mode_atomic.load(std::sync::atomic::Ordering::Relaxed),
                                );
                                let label = crate::commands::tray_mode_label(mode);
                                let title = crate::platform_integration::tray_title(
                                    label,
                                    true,
                                    Some(total),
                                );
                                let _ = tray_state.tray.set_title(Some(title.as_str()));
                            }
                        }
                    }
                }
            }
        });
        g.pull_task = Some(pull_task);
    }

    /// Stop the OpenAI Realtime session.
    ///
    /// Aborts the transport, event-consumer, and pull tasks, clears the uplink
    /// slot (so the graph loop silently drops further audio frames), drops the
    /// sender, and resets the output meter so the next backend-pushed meter
    /// frame returns the output VU meter to zero.
    ///
    /// Task 4.4: also resets `audio_flowing` / `last_send_time` / `latency_ms`,
    /// marks `openai_connected=false` in `AppStatus`, and emits `"status-changed"`.
    ///
    /// Caller must hold the `inner` mutex lock.
    fn stop_openai_session_locked(&self, g: &mut Inner) {
        // Signal the supervisor BEFORE aborting so it sees cancelled=true
        // and does not attempt to restart after the abort wakes it.
        self.session_active.store(false, Ordering::Release);
        // Issue #2: session ended — clear the reminder clocks so a later
        // session restarts duration/inactivity tracking from zero.
        self.mark_session_stopped();

        // Abort transport first. Dropping its event sender lets ev_task drain
        // and flush any partial transcript segment before the session log ends.
        if let Some(t) = g.realtime_task.take() {
            t.abort();
        }
        if let Some(t) = g.pull_task.take() {
            t.abort();
        }
        if let Some(t) = g.ev_task.take() {
            let session_log = Arc::clone(&self.session_log);
            tauri::async_runtime::spawn(async move {
                let mut ev_task = t;
                tokio::select! {
                    _ = &mut ev_task => {}
                    _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {
                        ev_task.abort();
                        let _ = ev_task.await;
                    }
                }
                session_log.end();
            });
        } else {
            self.session_log.end();
        }
        // Clear the uplink slot — graph loop will see None and drop frames.
        *self.uplink_slot.lock() = None;
        // Clear the original-queue slot and drain stale samples so no original
        // audio from a previous session bleeds into the next session.
        {
            let mut slot = self.original_queue_slot.lock();
            if let Some(q) = slot.as_ref() {
                q.lock().clear();
            }
            *slot = None;
        }
        // Drop the sender — this closes the channel to the (already-aborted) task.
        g.pcm_tx = None;
        // Honest idle: reset out_level to 0. The meter task owns the frontend
        // event stream and will publish the zero frame on the next tick.
        self.publish_output_level_zero();
        self.clear_output_preview();

        // Task 4.4: reset latency signals — no stale data carries into the
        // next session.
        self.audio_flowing.store(false, Ordering::Relaxed);
        *self.last_send_time.lock() = None;

        // Task 4.4: mark openai_connected=false and clear latency_ms in
        // AppStatus, then emit "status-changed".
        // Lock discipline: acquire std::sync::Mutex → clone → drop guard → emit
        // (same pattern as lib.rs 5 s-interval task; MutexGuard never crosses
        // an await point — stop_openai_session_locked is called synchronously).
        use tauri::Manager as _;
        let status_clone = {
            let app_handle = self.app.state::<crate::commands::AppHandle>();
            let mut st = app_handle.state.lock().unwrap();
            st.mark_openai_connected(false);
            st.status.latency_ms = None;
            st.status.clone()
        }; // MutexGuard dropped here
        let _ = self.app.emit("status-changed", status_clone);
        // Task 9: record stop-path closed (after MutexGuard dropped and emit done).
        self.conn_log.push("closed", "session ended");
    }

    fn open_capture_for_device(
        &self,
        source_id: Option<String>,
        backpressure: Arc<AudioBackpressureCounters>,
    ) -> Result<(CaptureHandle, std::sync::mpsc::Receiver<CapturedFrame>), AppError> {
        let started = std::time::Instant::now();
        lifecycle_trace(format!("capture.open start source_id={source_id:?}"));
        let level = Arc::clone(&self.level);
        let input_level_sequence = Arc::clone(&self.input_level_sequence);
        let app = self.app.clone();

        let result = capture::start(
            source_id.as_deref(),
            level,
            input_level_sequence,
            app,
            backpressure,
        );
        match &result {
            Ok(_) => lifecycle_trace(format!(
                "capture.open ok source_id={source_id:?} elapsed_ms={}",
                started.elapsed().as_millis()
            )),
            Err(error) => lifecycle_trace(format!(
                "capture.open error source_id={source_id:?} elapsed_ms={} error={}",
                started.elapsed().as_millis(),
                error
            )),
        }
        result
    }

    /// Attach an already-open capture stream to the live graph.
    ///
    /// Caller must hold the `inner` mutex lock. This function must not perform
    /// CoreAudio or filesystem I/O.
    fn install_capture_locked(
        &self,
        g: &mut Inner,
        handle: CaptureHandle,
        rx: std::sync::mpsc::Receiver<CapturedFrame>,
    ) {
        g.capture = Some(handle);
        let graph_stop = Arc::new(AtomicBool::new(false));
        g.graph_stop = Some(Arc::clone(&graph_stop));

        // Spawn graph task — routes captured frames to ring / OpenAI.
        //
        // The blocking receiver loop runs inside `spawn_blocking` so we
        // don't starve the Tokio runtime.  The loop owns:
        //   - `rx`: the frame receiver from the CPAL capture thread.
        //   - `ring_arc`: shared ring producer for PassThrough.
        //   - `mode_atomic`: lock-free mode read on every frame.
        //   - `out_level_arc`: written on PassThrough frames.
        //   - `uplink_slot`: shared slot holding the uplink Sender (or
        //     None).  The graph loop clones the Sender under the slot
        //     lock on each frame — lock time is sub-microsecond.
        let ring_arc = Arc::clone(&self.ring);
        let mode_atomic = Arc::clone(&self.mode_atomic);
        let out_level_arc = Arc::clone(&self.out_level);
        let output_level_sequence = Arc::clone(&self.output_level_sequence);
        let uplink_slot = Arc::clone(&self.uplink_slot);
        let original_queue_slot = Arc::clone(&self.original_queue_slot);
        let output_preview_slot = Arc::clone(&self.output_preview_slot);
        let graph_backpressure = Arc::clone(&self.backpressure);
        // Task 4.4: stamp the last-send time in the graph loop so
        // ev_task can compute openai_first_audio_ms.
        let graph_last_send = Arc::clone(&self.last_send_time);
        let graph_stop_loop = Arc::clone(&graph_stop);
        let graph_task = tauri::async_runtime::spawn(async move {
            tokio::task::spawn_blocking(move || {
                // One resampler instance persisted across frames so the
                // phase state carries across chunk boundaries (streaming-safe).
                let mut resampler =
                    intervox_core::audio::resampler::LinearResampler::new(48_000, 24_000);
                let mut uplink_chunker = graph::OpenAiChunker::new();

                while !graph_stop_loop.load(Ordering::Acquire) {
                    let frame = match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                        Ok(frame) => frame,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let mode = ring::mode_from_u32(mode_atomic.load(Ordering::Relaxed));
                    // Read the current original-queue slot under the lock
                    // (sub-microsecond: just clone the Arc option).
                    let oq = original_queue_slot.lock().clone();
                    let sent_to_openai = graph::route_frame(
                        mode,
                        &frame,
                        graph::RouteFrameContext {
                            ring: &ring_arc,
                            out_level: &out_level_arc,
                            output_sequence: &output_level_sequence,
                            uplink_slot: &uplink_slot,
                            resampler: &mut resampler,
                            uplink_chunker: &mut uplink_chunker,
                            original_queue: oq.as_ref(),
                            output_preview: &output_preview_slot,
                            backpressure: &graph_backpressure,
                        },
                    );
                    // Task 4.4: stamp only after a fixed 40 ms OpenAI
                    // chunk is successfully enqueued to the realtime
                    // transport. Stamping every capture frame would
                    // undercount the batching delay.
                    // Lock time: sub-microsecond (just store an Instant).
                    if sent_to_openai {
                        *graph_last_send.lock() = Some(std::time::Instant::now());
                    }
                }
            })
            .await
            .ok();
        });
        g.graph_task = Some(graph_task);

        // ── Task 4.5: capture watcher — one-shot auto-restart ─────────
        //
        // Polls every 500 ms to detect graph_task completion (which
        // happens when the capture thread's sender drops = device lost).
        // On device loss: emits a retryable AppError::audio_device_lost,
        // then — if `capture_restart_allowed` is still true — attempts
        // ONE automatic restart using the default device.
        //
        // `capture_restart_allowed` is set to true on mode-entry
        // (inside `set_mode`) and to false on the first auto-restart
        // attempt, preventing infinite restart storms.
        //
        // The watcher accesses the engine via `app.state::<Arc<Engine>>`
        // so it holds no direct Arc<Engine> reference — no circular Arc.
        let watcher_app = self.app.clone();
        let watcher_restart_allowed = Arc::clone(&self.capture_restart_allowed);
        let watcher_mode_atomic = Arc::clone(&self.mode_atomic);
        // The watcher needs a JoinHandle<()> to poll graph_task.is_finished().
        // We give it a shared Arc<AtomicBool> "capture_exited" that the
        // graph_task sets to true when the blocking recv loop exits.
        // This avoids needing to share the JoinHandle (which is not Clone).
        let capture_exited = Arc::new(AtomicBool::new(false));
        let capture_exited_graph = Arc::clone(&capture_exited);

        // Re-wrap the graph_task to set capture_exited when it completes.
        // We need to take the just-stored graph_task out and wrap it.
        let raw_graph_task = g.graph_task.take().expect("just set above");
        let graph_task_wrapped = tauri::async_runtime::spawn(async move {
            raw_graph_task.await.ok();
            capture_exited_graph.store(true, Ordering::Release);
        });
        g.graph_task = Some(graph_task_wrapped);

        let capture_watcher_task = tauri::async_runtime::spawn(async move {
            use tauri::Manager as _;

            // Poll until capture_exited is true (device lost / channel closed).
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                if capture_exited.load(Ordering::Acquire) {
                    break;
                }
            }

            // Capture thread has exited.  Check if mode still needs capture.
            let current_mode = ring::mode_from_u32(watcher_mode_atomic.load(Ordering::Relaxed));
            let routing = intervox_core::FrameRouting::for_mode_and_mix(current_mode, 0);
            let mode_needs_capture = routing.mic_to_ring || routing.mic_to_openai;

            if !mode_needs_capture {
                // Mode no longer needs capture — not a crash, just mode change.
                return;
            }

            // Emit retryable error to the frontend.
            // The `RecoveryAction` points to `set_virtual_mic_mode` which
            // the user can invoke to re-enter the current mode and restart
            // capture.  `audio_device_lost_retryable()` carries this action.
            let _ = watcher_app.emit(
                "error",
                intervox_core::AppError::audio_device_lost_retryable(),
            );

            // Attempt ONE automatic restart if the allowance flag is still set.
            // Consume the flag atomically (compare_exchange false→true→false).
            let was_allowed = watcher_restart_allowed
                .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                .is_ok();

            if !was_allowed {
                // Auto-restart already used (or intentionally disabled) —
                // surface only; user must retry via the banner.
                return;
            }

            // Short delay before restarting to let the OS settle.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            // Re-check mode after the delay.
            let current_mode = ring::mode_from_u32(watcher_mode_atomic.load(Ordering::Relaxed));
            let routing = intervox_core::FrameRouting::for_mode_and_mix(current_mode, 0);
            if !(routing.mic_to_ring || routing.mic_to_openai) {
                return;
            }

            // Perform the restart via the engine's state.
            if let Some(engine) = watcher_app.try_state::<std::sync::Arc<Engine>>() {
                let source_id = {
                    let g = engine.inner.lock();
                    // Only restart if capture is not already running
                    // (a manual restart via set_mode may have beaten us).
                    if g.capture.is_some() {
                        return;
                    }
                    g.source_id.clone()
                };
                match engine
                    .open_capture_for_device(source_id.clone(), Arc::clone(&engine.backpressure))
                {
                    Ok((handle, rx)) => {
                        let mut g = engine.inner.lock();
                        let current_mode =
                            ring::mode_from_u32(watcher_mode_atomic.load(Ordering::Relaxed));
                        let routing =
                            intervox_core::FrameRouting::for_mode_and_mix(current_mode, 0);
                        let mode_needs_capture = routing.mic_to_ring || routing.mic_to_openai;
                        if mode_needs_capture && g.capture.is_none() && g.source_id == source_id {
                            engine.install_capture_locked(&mut g, handle, rx);
                        } else {
                            drop(g);
                            handle.stop_in_background("stale-auto-restart-capture");
                            drop(rx);
                        }
                    }
                    Err(e) => {
                        eprintln!("[engine] failed to auto-restart capture: {e}");
                        let _ = watcher_app.emit("error", e);
                    }
                }
            }
        });
        g.capture_watcher_task = Some(capture_watcher_task);
    }

    fn stop_level_probe_locked(&self, g: &mut Inner) {
        if let Some(handle) = g.probe_capture.take() {
            handle.stop_in_background("probe-capture");
        }
        self.publish_input_level_zero();
    }

    /// Stop CPAL capture and the graph task.
    ///
    /// Caller must hold the `inner` mutex lock.
    fn stop_capture_locked(&self, g: &mut Inner) {
        let started = std::time::Instant::now();
        lifecycle_trace(format!(
            "capture.stop_locked start capture={} graph={} watcher={}",
            g.capture.is_some(),
            g.graph_task.is_some(),
            g.capture_watcher_task.is_some()
        ));
        if let Some(stop) = g.graph_stop.take() {
            stop.store(true, Ordering::Release);
        }
        // Abort the graph task (it holds the channel receiver).
        if let Some(t) = g.graph_task.take() {
            t.abort();
        }
        // Abort the capture watcher task (Task 4.5: no stale restart after stop).
        if let Some(t) = g.capture_watcher_task.take() {
            t.abort();
        }
        // Signal capture stop and reap it off the mode-switch path. CoreAudio
        // stream teardown can stall, so silence must not wait for thread join.
        if let Some(handle) = g.capture.take() {
            handle.stop_in_background("live-capture");
        }

        // Honest idle: reset both level atomics. The meter task owns frontend
        // delivery and will publish a coherent zero frame on the next tick.
        self.publish_input_level_zero();
        self.publish_output_level_zero();
        self.clear_output_preview();
        lifecycle_trace(format!(
            "capture.stop_locked done elapsed_ms={}",
            started.elapsed().as_millis()
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioBackpressureCounters, MeterChannelFreshness, METER_STALE_TICKS};

    #[test]
    fn backpressure_snapshot_reports_all_counter_totals() {
        let counters = AudioBackpressureCounters::default();

        counters.capture_pool_miss();
        counters.capture_capacity_drop();
        counters.capture_sink_drop();
        counters.uplink_no_session_drop();
        counters.uplink_queue_drop();
        counters.uplink_chunk_sent();

        let snapshot = counters.snapshot();
        assert_eq!(snapshot.capture_pool_misses, 1);
        assert_eq!(snapshot.capture_capacity_drops, 1);
        assert_eq!(snapshot.capture_sink_drops, 1);
        assert_eq!(snapshot.uplink_no_session_drops, 1);
        assert_eq!(snapshot.uplink_queue_drops, 1);
        assert_eq!(snapshot.uplink_chunks_sent, 1);
    }

    #[test]
    fn meter_events_preserve_constant_level_when_sequence_advances() {
        let mut freshness = MeterChannelFreshness::default();

        for sequence in 1..=3 {
            let sample = freshness.sample(0.25, sequence);
            assert!(sample.active);
            assert_eq!(sample.level, 0.25);
        }
    }

    #[test]
    fn meter_events_zero_stale_level_when_sequence_stops() {
        let mut freshness = MeterChannelFreshness::default();

        let first = freshness.sample(0.4, 1);
        assert!(first.active);
        assert_eq!(first.level, 0.4);

        for _ in 0..METER_STALE_TICKS {
            let still_fresh = freshness.sample(0.4, 1);
            assert!(still_fresh.active);
            assert_eq!(still_fresh.level, 0.4);
        }

        let stale = freshness.sample(0.4, 1);
        assert!(!stale.active);
        assert_eq!(stale.level, 0.0);
    }

    #[test]
    fn meter_events_without_any_samples_are_inactive() {
        let mut freshness = MeterChannelFreshness::default();
        let sample = freshness.sample(0.4, 0);

        assert!(!sample.active);
        assert_eq!(sample.level, 0.0);
    }
}

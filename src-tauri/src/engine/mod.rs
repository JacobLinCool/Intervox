//! Live audio engine: mode control, CPAL mic capture, and input-level events.
//!
//! `Engine` owns the ring producer, the optional `CaptureHandle`, and the
//! optional level-emit task.  It is `Send + Sync` because:
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
pub mod realtime;
pub mod ring;
pub mod supervisor;
pub mod translate_chain;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use intervox_core::{
    audio::jitter_buffer::JitterBuffer, realtime::events::TranslationEvent,
    state::VirtualMicMode, Config,
};
use parking_lot::Mutex;
use tauri::Emitter;

use capture::CaptureHandle;
use ring::{mode_from_u32, mode_to_ring_u32, RingProducer};
use translate_chain::SharedOriginalQueue;

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

// ── Inner state ───────────────────────────────────────────────────────────────

struct Inner {
    mode: VirtualMicMode,
    source_device_id: Option<String>,
    source_language: String,
    target_language: String,
    /// The running capture thread handle.  `None` when capture is stopped.
    capture: Option<CaptureHandle>,
    /// Tokio task that emits `"input-level"` and `"output-level"` events ~20 Hz.
    level_task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Graph task that reads captured frames from the channel and routes them
    /// (PassThrough → ring write; Translate → resample + PCM16 → OpenAI uplink).
    graph_task: Option<tauri::async_runtime::JoinHandle<()>>,
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
}

// ── Engine ────────────────────────────────────────────────────────────────────

/// The central live-audio controller managed by Tauri.
pub struct Engine {
    inner: Mutex<Inner>,
    ring: Arc<RingProducer>,
    app: tauri::AppHandle,
    /// Shared RMS level written by the capture callback, read by the level task.
    level: Arc<AtomicU32>,
    /// Shared output RMS level written by the graph loop (PassThrough path),
    /// emitted as `"output-level"` by the 20 Hz level task.
    out_level: Arc<AtomicU32>,
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
    /// the graph loop (running in spawn_blocking) reads it on every frame.
    /// See module-level doc for the rationale.
    uplink_slot: UplinkSlot,
    /// Shared original-audio tap queue (Task 4.3): the graph loop pushes 48 kHz
    /// mono frames here when in `TranslateWithOriginal` mode; the pull task
    /// drains 480 samples per tick, delays them, and mixes them under the
    /// translated audio before writing to the ring.
    ///
    /// The queue is cleared (drained to zero) at session stop so no stale
    /// original audio carries over into the next session.
    ///
    /// The graph loop reads an `Option<SharedOriginalQueue>` from a shared
    /// slot (identical pattern to `uplink_slot`) so that no-original modes
    /// (`Translate`, `Silence`, `PassThrough`) incur zero cost: the slot is
    /// `None` when not mixing.
    original_queue_slot: Arc<Mutex<Option<SharedOriginalQueue>>>,

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

        let inner = Inner {
            mode: VirtualMicMode::Silence,
            source_device_id: cfg.audio.source_mic_id.clone(),
            source_language: cfg.translation.source_language.clone(),
            target_language: cfg.translation.target_language.clone(),
            capture: None,
            level_task: None,
            graph_task: None,
            realtime_task: None,
            ev_task: None,
            pull_task: None,
            pcm_tx: None,
            capture_watcher_task: None,
        };

        // Start in Silence mode.
        ring.set_mode(mode_to_ring_u32(VirtualMicMode::Silence));
        ring.flush_silence();

        Self {
            inner: Mutex::new(inner),
            ring,
            app,
            level: Arc::new(AtomicU32::new(0)),
            out_level: Arc::new(AtomicU32::new(0)),
            mode_atomic: Arc::new(AtomicU32::new(mode_to_ring_u32(VirtualMicMode::Silence))),
            uplink_slot: Arc::new(Mutex::new(None)),
            original_queue_slot: Arc::new(Mutex::new(None)),
            last_send_time: Arc::new(Mutex::new(None)),
            audio_flowing: Arc::new(AtomicBool::new(false)),
            session_active: Arc::new(AtomicBool::new(false)),
            capture_restart_allowed: Arc::new(AtomicBool::new(false)),
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
        // Update the atomic FIRST so the graph loop sees the new mode on the
        // next frame even before the inner lock is acquired.
        self.mode_atomic
            .store(mode_to_ring_u32(mode), Ordering::Relaxed);

        {
            let mut g = self.inner.lock();
            g.mode = mode;
        }

        let routing = intervox_core::FrameRouting::for_mode(mode);

        self.ring.set_mode(mode_to_ring_u32(mode));

        if routing.ring_silence {
            self.ring.flush_silence();
            // Honest idle: reset both level atomics to 0 when entering Silence.
            self.level.store(0, Ordering::Relaxed);
            self.out_level.store(0, Ordering::Relaxed);
            let _ = self.app.emit("input-level", 0.0f32);
            let _ = self.app.emit("output-level", 0.0f32);
        }

        let needs_capture = routing.mic_to_ring || routing.mic_to_openai;

        let mut g = self.inner.lock();
        if needs_capture && g.capture.is_none() {
            // Allow exactly one auto-restart per mode-entry (Task 4.5).
            self.capture_restart_allowed.store(true, Ordering::Relaxed);
            self.start_capture_locked(&mut g);
        } else if !needs_capture && g.capture.is_some() {
            // No auto-restart when capture is intentionally stopped.
            self.capture_restart_allowed.store(false, Ordering::Relaxed);
            self.stop_capture_locked(&mut g);
        }

        // ── OpenAI Realtime session lifecycle ─────────────────────────────────
        if routing.openai_connected && g.realtime_task.is_none() {
            self.start_openai_session_locked(&mut g);
        } else if !routing.openai_connected && g.realtime_task.is_some() {
            self.stop_openai_session_locked(&mut g);
        }
    }

    // ── Device / language setters ─────────────────────────────────────────────

    /// Store the selected source-mic device ID and restart capture if running.
    pub fn set_source_device(&self, id: String) {
        let mut g = self.inner.lock();
        g.source_device_id = Some(id);
        // Restart capture with the new device if it is currently running.
        if g.capture.is_some() {
            self.stop_capture_locked(&mut g);
            self.start_capture_locked(&mut g);
        }
    }

    /// Store the source and target language codes.
    ///
    /// If an OpenAI session is active, it is restarted so the new languages
    /// take effect immediately.  Capture is not interrupted.
    pub fn set_languages(&self, src: String, tgt: String) {
        let mut g = self.inner.lock();
        g.source_language = src;
        g.target_language = tgt;

        // If a realtime session is running, restart it with the new languages.
        // The capture task and graph task continue uninterrupted; only the
        // OpenAI transport is restarted and the uplink_slot is refreshed.
        if g.realtime_task.is_some() {
            self.stop_openai_session_locked(&mut g);
            self.start_openai_session_locked(&mut g);
        }
    }

    // ── Shutdown ──────────────────────────────────────────────────────────────

    /// Graceful shutdown: stop capture + level task, OpenAI session, then flush silence.
    pub fn shutdown(&self) {
        {
            let mut g = self.inner.lock();
            self.stop_openai_session_locked(&mut g);
            self.stop_capture_locked(&mut g);
        }
        self.ring.flush_silence();
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Current operating mode.
    #[allow(dead_code)]
    pub fn mode(&self) -> VirtualMicMode {
        self.inner.lock().mode
    }

    /// Shared handle to the ring producer (for the audio pipeline).
    #[allow(dead_code)]
    pub fn ring(&self) -> Arc<RingProducer> {
        Arc::clone(&self.ring)
    }

    // ── Internal helpers (called with `inner` lock held) ──────────────────────

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
        let key = match crate::secrets::get_key() {
            Some(k) => k,
            None => {
                let _ = self.app.emit(
                    "error",
                    intervox_core::AppError::openai_auth_error("No API key set"),
                );
                return;
            }
        };

        let src_lang = g.source_language.clone();
        let tgt_lang = g.target_language.clone();

        // Read session-level config at session-start.
        // Using `load_or_default()` reads the file-backed config once so that
        // toggling settings takes effect on the next session start (consistent
        // with the established pattern for languages/limiter).
        let cfg = crate::appcfg::load_or_default();
        let limiter_enabled = cfg.audio.limiter_enabled;

        // Read current mode to decide whether to enable the original-audio mix.
        let current_mode = self.mode_atomic.load(std::sync::atomic::Ordering::Relaxed);
        let mix_original =
            intervox_core::FrameRouting::for_mode(ring::mode_from_u32(current_mode)).mix_original;

        // Build MixSettings from config (Task 4.3).
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
        // Capacity 64 keeps at most ~1.3 s of audio queued (at 50 ms chunks)
        // before frames are dropped.  Drops are acceptable; latency is not.
        let (pcm_tx, pcm_rx) = tokio::sync::mpsc::channel::<Vec<i16>>(64);

        // Event channel: realtime transport → event consumer.
        let (ev_tx, mut ev_rx) = tokio::sync::mpsc::channel::<TranslationEvent>(128);

        // Write the sender into the shared slot so the graph loop (which may
        // already be running in spawn_blocking) starts forwarding immediately.
        *self.uplink_slot.lock() = Some(pcm_tx.clone());
        g.pcm_tx = Some(pcm_tx);

        // Shared jitter buffer: ev_task pushes, pull_task pulls.
        let jitter: SharedJitterBuf =
            Arc::new(Mutex::new(translate_chain::new_jitter_buffer()));

        // Task 4.3: original-audio tap queue.
        // Only created and wired for TranslateWithOriginal; stays None for Translate.
        // The graph loop reads this from `original_queue_slot` on every frame.
        let original_queue: Option<SharedOriginalQueue> = if mix_original {
            let q = translate_chain::new_original_queue();
            *self.original_queue_slot.lock() = Some(Arc::clone(&q));
            Some(q)
        } else {
            *self.original_queue_slot.lock() = None;
            None
        };

        // Mark session active before spawning so the supervisor sees the flag
        // immediately (avoids a race where the supervisor checks before the flag
        // is set and exits on the first iteration).
        self.session_active.store(true, Ordering::Release);
        let session_active_rt = Arc::clone(&self.session_active);

        // Spawn the supervisor task (Task 4.5).
        //
        // The supervisor wraps `realtime::run` so that if `run` returns
        // unexpectedly (not because the session was stopped), it is restarted
        // with capped backoff.  Aborting the JoinHandle cancels the supervisor,
        // which propagates to the currently-awaited `run` future.
        let rt_task = tauri::async_runtime::spawn(supervisor::run_supervised(
            key,
            src_lang,
            tgt_lang,
            pcm_rx,
            ev_tx,
            session_active_rt,
        ));
        g.realtime_task = Some(rt_task);

        // ── ev_task: downlink event consumer ──────────────────────────────────
        //
        // Receives `TranslationEvent`s from the realtime transport and:
        //   - OutputAudioDelta → ingest into jitter buffer via translate_chain;
        //     record first-audio timing; set audio_flowing flag.
        //   - OutputTranscriptDelta → emit "target-transcript-delta" to frontend.
        //   - InputTranscriptDelta  → emit "source-transcript-delta" to frontend.
        //   - Error                 → emit "error" event to frontend.
        //   - SessionUpdated → mark openai_connected=true, emit "status-changed".
        //   - Closed → mark openai_connected=false, emit "status-changed".
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

            while let Some(ev) = ev_rx.recv().await {
                match ev {
                    TranslationEvent::SessionUpdated => {
                        // Task 4.4: session is confirmed live by the server.
                        // Mark openai_connected=true in AppStatus and emit
                        // "status-changed" — mirrors the lib.rs 5 s-interval
                        // lock/clone/emit discipline (MutexGuard dropped before emit).
                        use tauri::Manager as _;
                        let status_clone = {
                            let app_handle =
                                ev_app.state::<crate::commands::AppHandle>();
                            // std::sync::Mutex — must not hold across await points.
                            let mut st = app_handle.state.lock().unwrap();
                            st.mark_openai_connected(true);
                            st.status.clone()
                        }; // MutexGuard dropped here — safe to emit
                        let _ = ev_app.emit("status-changed", status_clone);
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
                    }

                    TranslationEvent::InputTranscriptDelta { text, .. } => {
                        // Emit { text } to match the frontend's
                        // `listen<{ text: string }>("source-transcript-delta", ...)`.
                        let _ = ev_app.emit(
                            "source-transcript-delta",
                            serde_json::json!({ "text": text }),
                        );
                    }

                    TranslationEvent::Error { message, .. } => {
                        // Surface to the UI; reconnect is handled inside realtime::run.
                        let _ = ev_app.emit(
                            "error",
                            intervox_core::AppError::network_error(message),
                        );
                    }

                    TranslationEvent::Closed => {
                        // Task 4.4: transport closed (network drop / server reset).
                        // Mark openai_connected=false — mirrors stop path.
                        // realtime::run handles reconnect; we just update the UI.
                        use tauri::Manager as _;
                        let status_clone = {
                            let app_handle =
                                ev_app.state::<crate::commands::AppHandle>();
                            let mut st = app_handle.state.lock().unwrap();
                            st.mark_openai_connected(false);
                            st.status.clone()
                        }; // MutexGuard dropped here
                        let _ = ev_app.emit("status-changed", status_clone);
                    }

                    TranslationEvent::Ignored(_) => {
                        // Unknown event type — silently ignore.
                    }
                }
            }
        });
        g.ev_task = Some(ev_task);

        // ── pull_task: 10 ms tick → (mix or limiter) → ring ──────────────────
        //
        // Pulls 480 frames every 10 ms from the jitter buffer.
        //
        // Translate (mix_original == false):
        //   translated_block → optional limiter → ring.write → out_level
        //
        // TranslateWithOriginal (mix_original == true):
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
        let jitter_pull = Arc::clone(&jitter);
        // Task 4.4: signals for latency emit.
        let pull_app = self.app.clone();
        let pull_audio_flowing = Arc::clone(&self.audio_flowing);
        let pull_first_audio_ms = Arc::clone(&first_audio_ms);
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
                    // TranslateWithOriginal path:
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
                ring_arc.write(&final_block);
                // Update out_level from the FINAL block (mixed or limited).
                let bits = translate_chain::rms_bits(&final_block);
                out_level_arc.store(bits, Ordering::Relaxed);

                // Task 4.4: emit "latency-changed" at ~1 Hz (every 100 ticks).
                // Gate: only when connected AND audio has actually flowed.
                // audio_flowing=true implies openai is connected because ev_task
                // sets this flag only after receiving the first OutputAudioDelta,
                // which requires an active session.
                if tick_count.is_multiple_of(100) {
                    let flowing = pull_audio_flowing.load(Ordering::Relaxed);
                    if translate_chain::should_emit_latency(
                        /*openai_connected=*/ flowing,
                        /*audio_flowing=*/ flowing,
                    ) {
                        // virtual_mic_output_lag_ms: ring backlog in ms.
                        // ring_arc.backlog_ms() acquires the parking_lot::Mutex
                        // briefly (sub-microsecond: one atomic read).
                        let ring_backlog_ms = ring_arc.backlog_ms();

                        let mut metrics =
                            intervox_core::diagnostics::metrics::LatencyMetrics {
                                capture_to_send_ms:
                                    translate_chain::CAPTURE_TO_SEND_EST_MS,
                                openai_first_audio_ms: pull_first_audio_ms
                                    .load(Ordering::Relaxed),
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
                            let app_handle =
                                pull_app.state::<crate::commands::AppHandle>();
                            let mut st = app_handle.state.lock().unwrap();
                            st.status.latency_ms = Some(total);
                        } // MutexGuard dropped here — never held across await
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
    /// sender, and emits honest-idle level events so the output VU meter returns
    /// to zero.
    ///
    /// Task 4.4: also resets `audio_flowing` / `last_send_time` / `latency_ms`,
    /// marks `openai_connected=false` in `AppStatus`, and emits `"status-changed"`.
    ///
    /// Caller must hold the `inner` mutex lock.
    fn stop_openai_session_locked(&self, g: &mut Inner) {
        // Signal the supervisor BEFORE aborting so it sees cancelled=true
        // and does not attempt to restart after the abort wakes it.
        self.session_active.store(false, Ordering::Release);

        // Abort tasks first so they stop consuming from the channels.
        if let Some(t) = g.realtime_task.take() {
            t.abort();
        }
        if let Some(t) = g.ev_task.take() {
            t.abort();
        }
        if let Some(t) = g.pull_task.take() {
            t.abort();
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
        // Honest idle: reset out_level to 0 and emit a final "output-level" 0.0
        // so the UI VU meter returns to zero rather than sticking at the last
        // translated-audio reading.
        self.out_level.store(0, Ordering::Relaxed);
        let _ = self.app.emit("output-level", 0.0f32);

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
    }

    /// Start CPAL capture and the associated level-emit + graph tasks.
    ///
    /// Caller must hold the `inner` mutex lock.
    fn start_capture_locked(&self, g: &mut Inner) {
        let device_id = g.source_device_id.as_deref().map(str::to_owned);
        let level = Arc::clone(&self.level);
        let app = self.app.clone();

        match capture::start(device_id.as_deref(), level, app) {
            Ok((handle, rx)) => {
                g.capture = Some(handle);

                // Spawn ~20 Hz level-emit task — emits BOTH input and output levels.
                let level_arc = Arc::clone(&self.level);
                let out_level_arc = Arc::clone(&self.out_level);
                let level_app = self.app.clone();
                let level_task = tauri::async_runtime::spawn(async move {
                    let mut tick =
                        tokio::time::interval(std::time::Duration::from_millis(50));
                    loop {
                        tick.tick().await;
                        let in_rms = f32::from_bits(level_arc.load(Ordering::Relaxed));
                        let out_rms = f32::from_bits(out_level_arc.load(Ordering::Relaxed));
                        let _ = level_app.emit("input-level", in_rms);
                        let _ = level_app.emit("output-level", out_rms);
                    }
                });
                g.level_task = Some(level_task);

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
                let uplink_slot = Arc::clone(&self.uplink_slot);
                let original_queue_slot = Arc::clone(&self.original_queue_slot);
                // Task 4.4: stamp the last-send time in the graph loop so
                // ev_task can compute openai_first_audio_ms.
                let graph_last_send = Arc::clone(&self.last_send_time);
                let graph_task = tauri::async_runtime::spawn(async move {
                    tokio::task::spawn_blocking(move || {
                        // One resampler instance persisted across frames so the
                        // phase state carries across chunk boundaries (streaming-safe).
                        let mut resampler =
                            intervox_core::audio::resampler::LinearResampler::new(48_000, 24_000);

                        while let Ok(frame) = rx.recv() {
                            let mode =
                                mode_from_u32(mode_atomic.load(Ordering::Relaxed));
                            // Read the current original-queue slot under the lock
                            // (sub-microsecond: just clone the Arc option).
                            let oq = original_queue_slot.lock().clone();
                            graph::route_frame(
                                mode,
                                &frame,
                                &ring_arc,
                                &out_level_arc,
                                &uplink_slot,
                                &mut resampler,
                                oq.as_ref(),
                            );
                            // Task 4.4: if the mode needs OpenAI uplink, stamp the
                            // last-send time so ev_task can compute first-audio
                            // latency.  We stamp on every translate frame — the
                            // most recent stamp before the first OutputAudioDelta
                            // arrives gives the best "send → first response" delta.
                            // Lock time: sub-microsecond (just store an Instant).
                            if intervox_core::FrameRouting::for_mode(mode).mic_to_openai {
                                *graph_last_send.lock() =
                                    Some(std::time::Instant::now());
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
                    let current_mode =
                        ring::mode_from_u32(watcher_mode_atomic.load(Ordering::Relaxed));
                    let routing = intervox_core::FrameRouting::for_mode(current_mode);
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
                    let current_mode =
                        ring::mode_from_u32(watcher_mode_atomic.load(Ordering::Relaxed));
                    let routing = intervox_core::FrameRouting::for_mode(current_mode);
                    if !(routing.mic_to_ring || routing.mic_to_openai) {
                        return;
                    }

                    // Perform the restart via the engine's state.
                    if let Some(engine) =
                        watcher_app.try_state::<std::sync::Arc<Engine>>()
                    {
                        let mut g = engine.inner.lock();
                        // Only restart if capture is not already running
                        // (a manual restart via set_mode may have beaten us).
                        if g.capture.is_none() {
                            engine.start_capture_locked(&mut g);
                        }
                    }
                });
                g.capture_watcher_task = Some(capture_watcher_task);
            }
            Err(e) => {
                eprintln!("[engine] failed to start capture: {e}");
                let app = self.app.clone();
                let _ = app.emit("error", e);
            }
        }
    }

    /// Stop CPAL capture, the level-emit task, and the graph task.
    ///
    /// Caller must hold the `inner` mutex lock.
    fn stop_capture_locked(&self, g: &mut Inner) {
        // Abort the level-emit task first (it references the level Arc).
        if let Some(t) = g.level_task.take() {
            t.abort();
        }
        // Abort the graph task (it holds the channel receiver).
        if let Some(t) = g.graph_task.take() {
            t.abort();
        }
        // Abort the capture watcher task (Task 4.5: no stale restart after stop).
        if let Some(t) = g.capture_watcher_task.take() {
            t.abort();
        }
        // Drop the handle → sets stop flag + joins the capture thread.
        g.capture = None;

        // Honest idle: reset both level atomics to 0 and emit final zero
        // events so the UI VU meters return to zero rather than sticking at
        // the last captured value.
        self.level.store(0, Ordering::Relaxed);
        self.out_level.store(0, Ordering::Relaxed);
        let _ = self.app.emit("input-level", 0.0f32);
        let _ = self.app.emit("output-level", 0.0f32);
    }
}

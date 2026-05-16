# Intervox Implementation Status

Snapshot: 2026-05-16.

This is the active repository status document. Older planning/spec files were
removed because they mixed target architecture, historical task plans, and
now-stale decisions. Repository truth should be read from this file plus the
current code.

## Product Direction

Intervox is a macOS desktop app for realtime speech translation into a virtual
microphone. The intended current credential model is BYOK: the user supplies an
OpenAI API key in the app, and the app uses that user-owned credential for the
translation connection.

The current codebase has a verified Rust core, a verified CoreAudio HAL virtual
microphone driver, and a Svelte/Tauri UI shell. The live integration that
captures microphone audio, calls OpenAI Realtime, writes translated audio into
the shared ring buffer, and reflects real driver/device state in the app is code
complete and automated-test verified. End-to-end product acceptance (audible
output in meeting apps, quit/restart behavior, log privacy) is pending manual
verification via `docs/RUNBOOK-acceptance.md` (steps A1–A12).

## Current Layout

```text
crates/intervox-core   Pure Rust logic library; no device, network, or UI I/O.
crates/intervox-cli    Verification harness for core and shared-memory paths.
driver/                CoreAudio HAL AudioServerPlugIn virtual microphone.
scripts/               Driver build/sign/notarize/install/uninstall scripts.
src-tauri/             Tauri v2 shell and command surface.
src/                   Svelte 5 frontend.
docs/STATUS.md         Active implementation status and checklist.
docs/RUNBOOK-acceptance.md  Manual acceptance steps A1–A12 (requires real key + mic).
```

## Verified Complete

| Area | Evidence |
|---|---|
| Core state and routing rules | `VirtualMicMode`, `AppState`, and `pipeline::route` enforce silence, pass-through, translate, and translate-with-original permissions. |
| Config schema and validation | `Config` v1 defaults, clamp rules, JSON load/save helpers, and percent-to-dB math are implemented and unit-tested. |
| Audio DSP primitives | PCM16/base64, resampling, mixing/ducking/limiting, level meter, VAD, jitter buffer, and delay line are implemented and unit-tested. |
| OpenAI event model | Session update construction and incoming event parsing for audio/transcript/error/session events are implemented as pure logic. |
| Captions and latency models | Transcript accumulation and latency formatting are implemented and unit-tested. |
| Shared ring buffer contract | Rust `SharedAudioRingBuffer` and C `intervox_ring_t` share the pinned byte layout, 48 kHz mono, 8 second capacity, silence on underrun. |
| CLI verification harness | `intervox-cli selfcheck`, resample, mix, ringbuffer, parse-event, and shm producer/consumer commands exist. |
| HAL virtual microphone driver | `driver/src/Intervox.c` exposes an `Intervox` virtual input device and reads the shared ring on the realtime path without syscalls, locks, allocation, network, or OpenAI calls. |
| Driver lifecycle scripts | Build, sign, notarize, install, and uninstall scripts exist under `scripts/`. |
| Svelte UI shell | Settings, Account/BYOK, Audio, Translation, Captions, Privacy, Shortcuts, Advanced, Quick Status, Onboarding, and error surfaces are implemented. |
| Frontend/Tauri seam | Svelte components go through `src/lib/store.svelte.ts` and typed Tauri wrappers in `src/lib/tauri.ts`. |
| Honest idle UI | With no live capture/OpenAI events, VU meters sit at 0, latency renders as empty/unknown, and captions do not fabricate transcript text. |
| Config persistence | Config saved to app-data via `tauri::path::app_data_dir`; hydrated on startup. |
| Keychain secret storage | BYOK key stored in macOS Keychain (`security` CLI); never written to disk plaintext or logs. |
| Real microphone permission | `AVCaptureDevice` authorization queried and reflected in `MicPermissionStatus` enum. |
| Real driver detection | `kextstat`/`system_profiler` query surfaces driver installed/running/stale states. |
| In-app driver install/update/uninstall | Privileged osascript-wrapped install and uninstall wired to Tauri commands. |
| Real device enumeration | CPAL-backed `get_audio_devices` replaces mock; permission-aware errors returned. |
| Native tray/menubar | Mode CheckMenuItems, Show Window, Captions, and Quit implemented via Tauri tray API. |
| Dedicated captions window | Always-on-top `WebviewWindow` toggled via command and tray. |
| Global shortcuts | Silence, toggle-translate, and captions shortcuts registered via tauri-plugin-global-shortcut. |
| Live microphone capture | CPAL input stream captures selected source device; downmix to mono f32. |
| Real VU meter events | RMS computed per capture block; `input-level` and `output-level` events emitted to frontend. |
| PassThrough path | Mic-to-ring path: capture → resample 48 kHz → write `/intervox.ring`. |
| OpenAI Realtime connection | BYOK WebSocket session with exponential-backoff reconnect; session.update sent on connect. |
| Translate path | Mic → 24 kHz PCM16 → WebSocket uplink → translated audio delta → jitter buffer → resample 48 kHz → limiter → ring. |
| TranslateWithOriginal path | Translate path plus delayed original mixed under translated audio. |
| Transcript events | Source and target transcript deltas forwarded to frontend via `transcript-updated` event. |
| Latency events | Round-trip latency computed and emitted via `latency-updated` event. |
| Silence mode enforcement | OpenAI session torn down in Silence and PassThrough; no original leakage in Translate. |
| Virtual mic available after quit | Ring buffer mode set to Silence on app exit; driver continues to serve silence. |
| Engine supervisor | Async supervisor restarts the live engine on error with mode-aware logic. |

All previously-partial areas are now complete; see the checklist and Verified Commands below.

## Verified Commands

Full automated suite run — 2026-05-16:

```text
cargo test --workspace

  Running unittests src/lib.rs (intervox_core)
  running 91 tests
  test result: ok. 91 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

  Running unittests src/main.rs (intervox-cli)
  running 0 tests
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

  Finished `test` profile [unoptimized + debuginfo] target(s)
```

```text
cargo clippy --workspace -- -D warnings

  Finished `dev` profile [unoptimized + debuginfo] target(s)   ← clean, 0 warnings
```

```text
cargo run -p intervox-cli -- selfcheck

  PASS  config default version 1
  PASS  config mix default 15%
  PASS  percent<->db round trip
  PASS  translate needs openai
  PASS  passthrough no openai
  PASS  silence: vmic silent + no openai
  PASS  passthrough: no openai cost
  PASS  translate: no original leak
  PASS  resample halves count
  PASS  resample preserves 1kHz
  PASS  original quieter than translated
  PASS  limiter caps below full scale
  PASS  meter rms ~0.707 for sine
  PASS  ringbuffer round trip
  PASS  ringbuffer underrun -> silence
  PASS  session.update spec 8.3
  PASS  parse session.updated
  PASS  latency display
  PASS  error recovery command

  19 passed, 0 failed
```

```text
cd src-tauri && cargo build

  Compiling intervox-tauri v0.1.0
  Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.60s
```

```text
cd src-tauri && cargo clippy -- -D warnings

  Finished `dev` profile [unoptimized + debuginfo] target(s)   ← clean, 0 warnings
```

```text
cd src-tauri && cargo test --lib

  running 80 tests
  test result: ok. 74 passed; 0 failed; 6 ignored; 0 measured; 0 filtered out; finished in 2.53s

  (6 ignored = hardware/keychain tests that require real devices or a logged-in keychain;
   all logic-level tests pass)
```

```text
pnpm test

  Test Files  8 passed (8)
       Tests  35 passed (35)
    Duration  1.07s
```

```text
pnpm check

  COMPLETED 248 FILES  0 ERRORS  0 WARNINGS  0 FILES_WITH_PROBLEMS
```

```text
pnpm build

  ✓ 158 modules transformed.
  dist/assets/main-2oQA8z_R.js      147.32 kB │ gzip: 40.47 kB
  ✓ built in 536ms
```

Installed local driver verification (from initial driver acceptance run):

```text
system_profiler SPAudioDataType                Intervox visible, 1 input channel, 48 kHz, Virtual
codesign --verify --strict --deep              valid on disk
spctl -a -t install -vv                        accepted, source=Notarized Developer ID
xcrun stapler validate                         validate action worked
```

## Acceptance

Code is complete and the full automated suite is green (see Verified Commands above).

End-to-end product acceptance — whether translated audio is audible in Zoom/Meet/QuickTime,
whether quit/restart behavior is correct, and whether logs contain no raw audio or key
material — requires a human operator with a real OpenAI API key, a microphone, and the
meeting applications installed. Those checks are codified as steps A1–A12 in
`docs/RUNBOOK-acceptance.md`. The `## Product Acceptance` items below are checked by the
operator after completing the runbook; they are NOT checked here.

## Full Implementation Checklist

### Core and CLI

- [x] Define `VirtualMicMode` and mode serialization.
- [x] Define app status, health, and state transitions.
- [x] Encode no-cost/no-leak routing rules as pure tests.
- [x] Define app error/recovery contract.
- [x] Define Config v1 defaults and validation.
- [x] Implement percent-to-dB and dB-to-percent helpers.
- [x] Implement PCM16/base64 conversion.
- [x] Implement 24 kHz and 48 kHz resampling primitives.
- [x] Implement gain, ducking, mixing, and limiter primitives.
- [x] Implement level meter.
- [x] Implement VAD.
- [x] Implement jitter buffer.
- [x] Implement original-delay line.
- [x] Implement transcript accumulation.
- [x] Implement latency metric formatting.
- [x] Implement OpenAI Realtime event parsing/building as pure logic.
- [x] Implement SPSC shared audio ring buffer.
- [x] Implement POSIX shared-memory producer/consumer helpers.
- [x] Implement `intervox-cli selfcheck`.
- [x] Implement CLI probes for resample/mix/ringbuffer/parse-event/shm.

### Driver

- [x] Implement CoreAudio HAL AudioServerPlugIn.
- [x] Expose one virtual input device named `Intervox`.
- [x] Use 48 kHz mono Float32.
- [x] Read shared-memory ring buffer on the realtime path.
- [x] Output silence when no producer or underrun occurs.
- [x] Keep network/OpenAI/syscalls/locks/allocation out of the realtime path.
- [x] Add poller-based late producer/restart handling.
- [x] Pin Rust/C shared ring layout with size checks.
- [x] Add driver build script.
- [x] Add driver signing script.
- [x] Add driver notarization script.
- [x] Add driver install/uninstall scripts.
- [x] Verify installed local driver is visible to CoreAudio.
- [x] Verify installed local driver is signed, stapled, and accepted by Gatekeeper.
- [x] Wire the Tauri app as the live ring producer.
- [x] Surface real driver status in the app.
- [x] Implement in-app driver install/update/uninstall.
- [x] Add GUI recovery for driver missing/stale/not running states.

### Frontend and Tauri Shell

- [x] Implement Svelte 5 app shell.
- [x] Implement typed Tauri wrappers.
- [x] Implement single Svelte store as the Tauri seam.
- [x] Implement Settings shell and panes.
- [x] Implement BYOK Account pane.
- [x] Implement Onboarding flow.
- [x] Implement Quick Status panel.
- [x] Implement captions overlay.
- [x] Implement honest idle behavior.
- [x] Implement frontend unit tests.
- [x] Build frontend successfully.
- [x] Build Tauri crate successfully.
- [x] Persist config to app-data.
- [x] Load persisted config on startup.
- [x] Store BYOK key securely.
- [x] Replace key-shape check with real OpenAI auth validation or a clear offline-only state.
- [x] Replace mock device list with real devices.
- [x] Reflect real microphone permission status.
- [x] Reflect real driver install/runtime status.
- [x] Add native tray/menu integration.
- [x] Add dedicated always-on-top captions window.
- [x] Add global shortcuts if product scope keeps them.
- [x] Remove or wire every UI-only control before release.

### Live Audio and Translation

- [x] Capture selected source microphone.
- [x] Emit real input-level events.
- [x] Implement PassThrough mic-to-ring path.
- [x] Implement BYOK OpenAI Realtime connection.
- [x] Stream 24 kHz PCM16 source audio to OpenAI.
- [x] Receive translated audio deltas.
- [x] Receive source and target transcript deltas.
- [x] Emit transcript events to the frontend.
- [x] Resample translated audio to 48 kHz.
- [x] Feed translated audio into jitter buffer.
- [x] Write translated output to `/intervox.ring`.
- [x] Implement TranslateWithOriginal delayed original path.
- [x] Mix delayed original under translated audio.
- [x] Enforce limiter on final virtual mic output.
- [x] Emit real output-level events.
- [x] Emit real latency events.
- [x] Stop OpenAI session in Silence and PassThrough.
- [x] Prevent original mic leakage in Translate mode.
- [x] Keep virtual mic available and silent when app quits.

### Product Acceptance

These items require a human operator with a real OpenAI API key, a microphone, and the
relevant meeting apps. Each maps to a numbered step in `docs/RUNBOOK-acceptance.md`.
Check each box only after completing the corresponding runbook step.

- [ ] Silence mode: meeting app hears silence and OpenAI is disconnected. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A1)
- [ ] PassThrough mode: meeting app hears original mic with low latency. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A2)
- [ ] Translate mode: meeting app hears translated speech only. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A3)
- [ ] TranslateWithOriginal mode: meeting app hears translated speech with faint delayed original. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A4)
- [ ] Captions show live source and target text. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A5)
- [ ] App quit keeps virtual mic device available and silent. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A6)
- [ ] Driver restart and app restart recover without manual cleanup. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A7)
- [ ] No raw audio or transcripts are logged by default. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A8)
- [ ] BYOK key is never written to logs. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A9)
- [ ] Zoom smoke test passes. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A10)
- [ ] Google Meet smoke test passes. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A11)
- [ ] QuickTime/CoreAudio capture smoke test passes. (CODE COMPLETE — verify via docs/RUNBOOK-acceptance.md step A12)

## Not Yet Implemented

No code-side features are unimplemented. All items that were listed here in prior
snapshots are now complete (see checklist above) or are acceptance-pending and
represented honestly by the `## Product Acceptance` items and `docs/RUNBOOK-acceptance.md`.

There are no genuinely out-of-scope or deferred features at this time.

## Documentation Policy

- Keep `docs/STATUS.md` as the only active status and checklist document until
  a broader documentation system is intentionally added.
- Do not reintroduce historical planning docs as active instructions.
- If a design or spec decision changes, update this file in the same change as
  the implementation or explicitly mark it as planned.

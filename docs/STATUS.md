# Intervox Implementation Status

Snapshot: 2026-05-17.

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
complete and automated-test verified. The frontend real-wiring pass replaced
fake/dead UI controls with backend signals, persisted local state, and visible
action feedback. End-to-end product acceptance (audible output in meeting apps,
quit/restart behavior, log privacy) is pending manual
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
docs/ARCHITECTURE.md   Durable runtime architecture and invariants.
docs/DEVELOPMENT.md    Local setup, driver lifecycle, and verification commands.
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
| Local BYOK storage | BYOK key stored in the Intervox config file under app-data with user-only file permissions; never written to logs. |
| Translation connection signal | `AppStatus.translation` is driven by mode transitions and OpenAI Realtime events; sidebar chip and Status pane share it. |
| Local usage accounting | 24 kHz PCM16 samples successfully sent to OpenAI are folded into `usage.json`; Account pane shows current-month and lifetime estimates. |
| Transcript history | Finalized source/target transcript segments are saved as per-session local JSONL files when enabled; audio bytes are never stored. |
| Connection log | A 200-entry in-memory ring plus capped `connection.log` records lifecycle events without transcript text, audio bytes, or keys. |
| UI config and native integration | `Config.ui` drives menu-bar latency badge, launch-at-login LaunchAgent, and Dock-icon visibility. |
| External links and version display | HTTPS-only `open_external_url` backs OpenAI links; frontend reads the real Tauri app version instead of hardcoded build strings. |
| Action feedback | State-changing actions route through non-blocking toasts for success/failure feedback. |
| Real microphone permission | `AVCaptureDevice` authorization queried and reflected in `MicPermissionStatus` enum. |
| Real driver detection | Filesystem install state is cheap; CoreAudio visibility is derived from the app's bounded CPAL device snapshot. |
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
| Transcript events | Source and target transcript deltas forwarded to frontend via dedicated transcript events; completion events finalize saved segments. |
| Latency events | Round-trip latency computed and emitted via `latency-updated` event. |
| Silence mode enforcement | OpenAI session torn down in Silence and PassThrough; no original leakage in Translate. |
| Virtual mic available after quit | Ring buffer mode set to Silence on app exit; driver continues to serve silence. |
| Engine supervisor | Async supervisor restarts the live engine on error with mode-aware logic. |

All previously-partial areas are now complete; see the checklist and Verified Commands below.

## Verified Commands

Latest automated suite run — 2026-05-17:

```text
cargo test --workspace

  intervox-cli: 0 tests passed
  intervox-core: 115 tests passed
  intervox-core doc-tests: 0 tests passed
```

```text
cargo test --manifest-path src-tauri/Cargo.toml

  intervox-tauri-lib: 86 passed; 0 failed; 6 ignored
  intervox-tauri: 0 tests passed
  intervox-tauri-lib doc-tests: 0 tests passed
```

```text
cargo clippy --workspace -- -D warnings

  Finished clean, 0 warnings.
```

```text
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

  Finished clean, 0 warnings.
```

```text
pnpm test

  Test Files  8 passed (8)
       Tests  51 passed (51)
```

```text
pnpm check

  svelte-check found 0 errors and 0 warnings
```

```text
pnpm build

  ✓ 162 modules transformed.
  ✓ built in 760ms
```

```text
git diff --check

  no whitespace errors
```

Installed local driver verification (from initial driver acceptance run):

```text
App bounded device refresh                     Intervox visible as an input device
codesign --verify --strict --deep              valid on disk
spctl -a -t install -vv                        accepted, source=Notarized Developer ID
xcrun stapler validate                         validate action worked
```

## Acceptance

Code is complete and the full automated suite is green (see Verified Commands above).

End-to-end product acceptance — whether translated audio is audible in Zoom/Meet/QuickTime,
whether quit/restart behavior is correct, and whether runtime logs contain no raw audio,
transcript text, or key material — requires a human operator with a real OpenAI API key, a microphone, and the
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
- [x] Show current-month and lifetime local usage estimates.
- [x] Persist transcript history as local per-session JSONL when enabled.
- [x] Clear saved transcript files and live session transcript state from the UI.
- [x] Surface the connection log in the Advanced pane.
- [x] Persist and apply UI config for latency badge, launch-at-login, and Dock visibility.
- [x] Open OpenAI account/API-key links through the HTTPS-only native command.
- [x] Display the real app version from Tauri.
- [x] Show non-blocking feedback toasts for user-triggered mutations.
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

## Not Yet Implemented

No code-side features are unimplemented. All items that were listed here in prior
snapshots are now complete (see checklist above) or are acceptance-pending and
represented honestly by the `## Product Acceptance` items and `docs/RUNBOOK-acceptance.md`.

There are no genuinely out-of-scope or deferred features at this time.

## Documentation Policy

- Keep `README.md` as the project entry point.
- Keep durable architecture in `docs/ARCHITECTURE.md`.
- Keep local setup and command workflows in `docs/DEVELOPMENT.md`.
- Keep `docs/STATUS.md` as the active status and checklist document.
- Keep real-hardware acceptance in `docs/RUNBOOK-acceptance.md`.
- Do not reintroduce historical planning docs as active instructions.
- If a design or spec decision changes, update this file in the same change as
  the implementation or explicitly mark it as planned.

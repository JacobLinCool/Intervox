# Intervox Architecture

This document is the durable architecture record. Historical task plans should
not be used as implementation instructions once the code has moved on.

## Product Model

Intervox is a BYOK realtime speech-translation app for macOS. The user provides
an OpenAI API key, selects a physical microphone, and selects the Intervox
virtual input device in meeting apps.

The runtime has two separate responsibilities:

- The app process captures, translates, mixes, and writes audio.
- The CoreAudio HAL plug-in exposes a virtual input device and reads only from
  the shared ring buffer.

This separation keeps OpenAI networking and app control-plane work out of the
CoreAudio realtime path.

## Component Boundaries

| Component | Responsibility |
|---|---|
| `crates/intervox-core` | Pure Rust logic: config schema, routing rules, DSP helpers, OpenAI event parsing, captions, latency formatting, and shared ring layout. No device, network, or UI I/O. |
| `crates/intervox-cli` | Headless verification harness for core logic and shared-memory paths. |
| `driver/` | CoreAudio HAL AudioServerPlugIn. Exposes the virtual input device and serves 48 kHz mono Float32 audio from shared memory. |
| `src-tauri/` | Native macOS/Tauri shell: config persistence, microphone permission, device enumeration, driver install/status, tray, shortcuts, OpenAI Realtime, and live audio engine. |
| `src/` | Svelte UI. All Tauri IPC calls go through typed wrappers and the single app store. |
| `scripts/` | Build, sign, notarize, install, and uninstall operations for the HAL driver. |

## Runtime State Surfaces

The app exposes user-visible state only from implemented backend signals:

- `AppStatus.translation` is the translation-connection source of truth. It
  moves through `idle`, `connecting`, `connected`, `reconnecting`, and `failed`
  from mode transitions and OpenAI Realtime events. The sidebar chip and Status
  pane both render this field.
- Usage is a local estimate derived from 24 kHz PCM16 samples successfully sent
  to the OpenAI uplink. The app stores current-month and lifetime totals in
  `usage.json` and surfaces both values in the Account pane.
- Transcript history is a per-session local JSONL file under `transcripts/`.
  Only finalized source/target transcript segments are written. Audio bytes are
  never written.
- The connection log is a bounded in-memory ring plus a capped
  `connection.log` text file containing lifecycle events such as connecting,
  connected, reconnecting, failed, closed, and latency samples. It must not
  contain audio, API keys, or transcript text.
- `Config.ui` drives native integration: menu-bar latency badge, launch-at-login
  LaunchAgent, and Dock-icon visibility. These settings apply immediately.
- External links are opened through the Tauri command surface and only allow
  `https://` URLs.
- The frontend displays the Tauri app version from `@tauri-apps/api/app`; there
  is no hardcoded build number in the UI.

## Runtime Data Flow

```text
Physical mic
  -> CPAL capture
  -> intervox-core routing and DSP
  -> optional OpenAI Realtime session
  -> translated PCM
  -> jitter buffer / delay / mix / limiter
  -> POSIX shared-memory ring (/intervox.ring)
  -> CoreAudio HAL driver
  -> meeting app selected input
```

The engine uses the mode routing rules from `intervox-core`:

| Mode | Capture | OpenAI | Ring output |
|---|---:|---:|---|
| Silence | no | no | silence |
| Pass-through | yes | no | original mic audio |
| Interpret, original voice 0% | yes | yes | translated audio only |
| Interpret, original voice >0% | yes | yes | translated audio plus delayed quiet original |

Interpret at 0% original voice must not leak original microphone audio to the virtual mic.
Silence and Pass-through must not keep an OpenAI session open.

## HAL Driver Contract

The driver is a realtime audio component. Its render path must stay bounded and
predictable:

- No network calls.
- No OpenAI calls.
- No locks on the render path.
- No allocation on the render path.
- No filesystem or process spawning on the render path.
- Silence on missing producer, underrun, or shutdown.

Driver install, status checks, notarization, and recovery belong to scripts or
the Tauri control plane, not to the HAL realtime callback.

## Shared Ring Contract

Rust and C share a pinned ring-buffer layout. The layout is verified by tests on
both sides. The agreed format is:

- 48 kHz.
- Mono.
- Float32 samples.
- Eight seconds of capacity.
- Single app producer, CoreAudio consumer.

When no app producer is active, the driver serves silence so meeting apps keep a
stable microphone device without transmitting stale audio.

## Config and Credential Model

Config lives at:

```text
~/Library/Application Support/app.intervox.desktop/config.json
```

The config directory is mode `700`; the config file is mode `600`.

The OpenAI API key is stored in that config file under the account section. It
is validated through OpenAI before the app treats it as verified. The app does
not keep migration code for older credential locations.

Frontend config IPC intentionally strips the raw API key before sending config
state to Svelte.

## Onboarding Truth Sources

Onboarding must reflect operating-system truth, not button-click state.

- Microphone access comes from AVFoundation authorization state.
- Opening System Settings is not permission success.
- The Continue button is valid only when microphone state is `granted`.
- The virtual device is considered available only when the app's bounded CPAL
  device snapshot sees the Intervox input device.
- Driver install state on disk is separate from CoreAudio visibility.

The onboarding probe in `src-tauri/src/lib.rs` exists to check these surfaces
without printing secrets.

## Privacy Invariants

- Raw audio is not logged.
- Transcript history is saved locally by default as user-controlled JSONL files;
  it can be disabled or cleared from the Privacy/Advanced UI.
- Runtime logs and `connection.log` must not contain transcript text.
- OpenAI API keys are not logged.
- API key validation reports status only.
- Silence mode stops OpenAI activity and writes silence.
- App quit parks the ring in silence.

Manual verification for these invariants is captured in
`docs/RUNBOOK-acceptance.md`.

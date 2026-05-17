# Intervox

Intervox is a macOS desktop app that interprets live speech and exposes the
translated voice as a virtual microphone for any apps.

Speak normally. Intervox listens through your real microphone, translates what
you say, and makes the translated voice available as **Interpreter Mic**. Zoom,
Google Meet, QuickTime, and any app that can choose a microphone can use it like
a regular input device.

## What It Does

- Interprets live speech and sends the translated voice through a virtual microphone.
- Lets meeting apps hear the translated voice instead of the original mic.
- Supports silence, pass-through, translated voice, and translated voice mixed
  with the original.
- Shows captions, latency, connection state, usage estimates, and driver status.
- Keeps the OpenAI API key and local app data on the Mac.

## How It Works

Intervox runs as a Tauri desktop app with a small macOS audio driver. The app
captures a selected microphone, sends speech to OpenAI Realtime with the user's
own API key, receives translated audio, and writes it to the virtual microphone.

The product name shown in the UI is **Interpreter Mic**. macOS and some audio
selectors may show the underlying device as **Intervox**.

## Status

The app path for capture, translation, virtual-mic output, onboarding,
permission checks, and driver management is implemented.

The remaining release gate is hands-on acceptance on real macOS hardware with a
real microphone, OpenAI API key, and meeting apps. The manual checklist lives in
[docs/RUNBOOK-acceptance.md](docs/RUNBOOK-acceptance.md).

## Requirements

- macOS 14 Sonoma or later.
- Rust 1.94.0, pinned by `rust-toolchain.toml`.
- Xcode Command Line Tools.
- Node.js 24.2.0 and pnpm 10.21.0.
- `cmake` and `ninja` for the audio driver.
- OpenAI API key for live translation.
- Apple Developer ID Application certificate and notarytool credentials for
  signed release builds.

## Quick Start

Install dependencies:

```bash
pnpm install
```

Run the fast checks:

```bash
cargo test --workspace
pnpm test
pnpm check
```

Run the app in development mode:

```bash
pnpm tauri dev
```

When setup is incomplete, Intervox opens the first-run onboarding flow.

## Build

Build a local unsigned development app:

```bash
pnpm run build:app:dev
```

Build the signed and notarized release app:

```bash
pnpm run build:app
```

`build:app` is the release path. It requires Developer ID and notarytool
credentials because the app bundle includes the audio driver.

## Driver

Build the driver:

```bash
scripts/build_driver.sh
```

Sign and notarize it for release or full local acceptance:

```bash
scripts/sign_driver.sh
scripts/notarize_driver.sh
```

Install it locally:

```bash
INTERVOX_ASSUME_YES=1 sudo bash scripts/install_driver.sh
```

The app also supports privileged driver install, update, and uninstall from
onboarding and the Status pane.

## Local Data

Intervox stores settings and the OpenAI API key in:

```text
~/Library/Application Support/app.intervox.desktop/config.json
```

The config directory uses user-only permissions, and the config file is written
with mode `600`. The key is never returned to the frontend by the config IPC
command.

Other local files live in the same app-data directory:

| File or directory | Purpose |
|---|---|
| `usage.json` | Local month and lifetime usage estimate. |
| `transcripts/*.jsonl` | Per-session transcript history when transcript saving is enabled. |
| `connection.log` | Capped connection lifecycle log for troubleshooting. |

Do not commit local secret files such as `apikey.secret` or `password.secret`.

## Acceptance

Before calling a build release-ready, run the full manual runbook:

```text
docs/RUNBOOK-acceptance.md
```

The runbook checks audio modes, translated voice, original-voice mix, captions,
quit and restart behavior, privacy and logging, plus smoke tests in Zoom, Google
Meet, and QuickTime.

## Docs

| Document | Purpose |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Runtime model, component boundaries, privacy model, and audio-driver invariants. |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Local setup, driver lifecycle, build/test commands, and probes. |
| [docs/STATUS.md](docs/STATUS.md) | Current implementation status and acceptance checklist. |
| [docs/RUNBOOK-acceptance.md](docs/RUNBOOK-acceptance.md) | Manual A1-A12 acceptance flow for real hardware and meeting apps. |

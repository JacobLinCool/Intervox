# Intervox

Intervox is a macOS desktop app that translates live speech and exposes the
result as a CoreAudio virtual microphone for meeting apps.

The app captures a selected physical microphone, uses the user's OpenAI API key
to run realtime translation, and writes the translated audio into a HAL virtual
input device. Meeting apps then select that virtual input instead of the
physical microphone.

## Current Status

The code path for capture, translation, virtual-mic output, onboarding,
permission checks, and driver management is implemented. The remaining release
gate is manual product acceptance on real macOS hardware with a real microphone,
OpenAI API key, and meeting apps.

Authoritative docs:

| Document | Purpose |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Runtime model, component boundaries, privacy and HAL invariants. |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Local setup, driver lifecycle, build/test commands, and probes. |
| [docs/STATUS.md](docs/STATUS.md) | Current implementation status and acceptance checklist. |
| [docs/RUNBOOK-acceptance.md](docs/RUNBOOK-acceptance.md) | Manual A1-A12 acceptance flow for real hardware and meeting apps. |

## Requirements

- macOS 14 Sonoma or later.
- Rust 1.94.0, pinned by `rust-toolchain.toml`.
- Xcode Command Line Tools.
- Node.js 24.2.0 plus pnpm 10.21.0.
- `cmake` and `ninja` for the HAL driver.
- Apple Developer ID Application certificate for signed app and driver builds.
- Apple notarytool credentials for notarized release app and driver builds.
- OpenAI API key for live translation.

## Quick Start

Install JavaScript dependencies:

```bash
pnpm install
```

Run the fast logic and frontend checks:

```bash
cargo test --workspace
pnpm test
pnpm check
```

Run the Tauri app in development mode:

```bash
pnpm tauri dev
```

The app will open the first-run onboarding flow when setup is incomplete.

Build the packaged macOS app:

```bash
pnpm run build:app
```

`build:app` is the release path and requires Developer ID plus notarytool
credentials because the bundled driver must pass the app's install trust gate.
For a local unsigned development package:

```bash
pnpm run build:app:dev
```

Build a release candidate explicitly:

```bash
pnpm run build:app:release
```

## Driver Lifecycle

Build the HAL driver:

```bash
scripts/build_driver.sh
```

The app build script runs the driver build before packaging because the app
bundle includes `driver/build/Intervox.driver` as a resource.

Sign and notarize the driver before release or full local acceptance:

```bash
scripts/sign_driver.sh
scripts/notarize_driver.sh
```

Install the driver:

```bash
INTERVOX_ASSUME_YES=1 sudo bash scripts/install_driver.sh
```

The app also exposes an in-app privileged install path during onboarding and in
the Status pane.

Product UI calls the virtual input **Translator Mic**. CoreAudio may expose the
device name as **Intervox** in system and meeting-app selectors.

## Config and API Key

Intervox stores app settings and the OpenAI API key in:

```text
~/Library/Application Support/app.intervox.desktop/config.json
```

The config directory is written with user-only permissions and the config file
is written with mode `600`. The OpenAI API key is stored in this local config
file and is not returned to the frontend by the config IPC command.

Other local app data is stored under the same app-data directory:

| File or directory | Purpose |
|---|---|
| `usage.json` | Local month and lifetime usage estimate from translation audio sent to OpenAI. |
| `transcripts/*.jsonl` | Per-session source/target transcript history when transcript saving is enabled. |
| `connection.log` | Capped connection lifecycle log for troubleshooting; no audio, transcript text, or keys. |

Do not commit local secret files such as `apikey.secret` or `password.secret`.

## Manual Acceptance

Before calling a build release-ready, run the full manual runbook:

```text
docs/RUNBOOK-acceptance.md
```

The runbook checks all four audio modes, captions, quit/restart behavior,
privacy/logging, and smoke tests in Zoom, Google Meet, and QuickTime.

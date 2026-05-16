# Intervox Development

This document is the working guide for local development. `README.md` stays as
the entry point; this file holds commands and operational detail.

## Repository Layout

```text
crates/intervox-core   Pure Rust logic and tests.
crates/intervox-cli    Core/shared-memory verification CLI.
driver/                CoreAudio HAL AudioServerPlugIn.
scripts/               Driver lifecycle scripts.
src-tauri/             Tauri v2 native app shell and live engine.
src/                   Svelte 5 frontend.
docs/                  Architecture, status, and acceptance docs.
```

`src-tauri` is intentionally excluded from the root Cargo workspace because it
pulls GUI and platform dependencies. Use root Cargo commands for core logic and
`--manifest-path src-tauri/Cargo.toml` for the app crate.

## Setup

Install dependencies:

```bash
pnpm install
```

Confirm local toolchain basics:

```bash
rustup show
xcode-select -p
cmake --version
ninja --version
pnpm --version
```

## Main Development Commands

Core workspace:

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo run -p intervox-cli -- selfcheck
```

Tauri crate:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Frontend:

```bash
pnpm test
pnpm check
pnpm build
```

Run the app:

```bash
pnpm tauri dev
```

Build the app bundle:

```bash
pnpm run build:app
```

Build a release candidate with checks, driver notarization, app notarization,
stapling, and Gatekeeper validation:

```bash
pnpm run build:app:release
```

## Driver Build, Signing, and Install

Build:

```bash
scripts/build_driver.sh
```

Sign:

```bash
scripts/sign_driver.sh
```

Notarize and staple:

```bash
scripts/notarize_driver.sh
```

The app build script runs `scripts/build_driver.sh` and `scripts/sign_driver.sh`
before invoking Tauri so the bundled driver resource exists and is signed.
`pnpm run build:app:release` also notarizes/staples the driver before packaging.

Install:

```bash
INTERVOX_ASSUME_YES=1 sudo bash scripts/install_driver.sh
```

Uninstall:

```bash
INTERVOX_ASSUME_YES=1 sudo bash scripts/uninstall_driver.sh
```

After install or uninstall, CoreAudio is restarted. Audio on the machine may be
briefly interrupted.

Avoid `system_profiler SPAudioDataType` during HAL debugging. It can block
inside CoreAudio when a plug-in is unhealthy. Prefer the app's bounded device
refresh, Audio MIDI Setup, or CPAL enumeration.

## Onboarding Probe

The Tauri binary has a non-UI onboarding probe for local validation:

```bash
cargo run --manifest-path src-tauri/Cargo.toml -- \
  --intervox-onboarding-probe \
  --out /tmp/intervox-onboarding-probe.json \
  --api-key-file "$PWD/apikey.secret"
```

Use `--request-mic` only when you intentionally want the probe to trigger the
macOS microphone permission prompt.

The probe reports booleans and status values. It must not print the raw OpenAI
API key.

## Config Location

The app uses the app-data directory:

```text
~/Library/Application Support/app.intervox.desktop/
```

Important files:

| Path | Notes |
|---|---|
| `config.json` | Main config plus BYOK OpenAI API key; written mode `600`. |
| `usage.json` | Local usage estimate; folded from successful OpenAI uplink samples every ~10 seconds and at shutdown. |
| `transcripts/*.jsonl` | Per-session finalized transcript segments when transcript history is enabled; directory mode `700`, files mode `600`. |
| `connection.log` | Capped text log of translation connection lifecycle events; mode `600`. |

Expected config permissions:

```bash
stat -f %Lp "$HOME/Library/Application Support/app.intervox.desktop"
stat -f %Lp "$HOME/Library/Application Support/app.intervox.desktop/config.json"
```

The directory should be `700`; the file should be `600`.

Launch-at-login uses the user LaunchAgent:

```text
~/Library/LaunchAgents/app.intervox.desktop.plist
```

The app creates or removes that file when the Advanced pane's "Launch at login"
toggle changes.

## Verification Before Handoff

For a code change, run the smallest relevant checks first, then run the broader
gate when touching shared behavior:

```bash
cargo test --workspace
cargo test --manifest-path src-tauri/Cargo.toml
pnpm test
pnpm check
git diff --check
```

Run the full driver and acceptance flow when changes affect:

- `driver/`
- `scripts/`
- shared-memory ring layout
- app startup or shutdown behavior
- microphone permission
- virtual device install/status
- OpenAI Realtime audio routing

Manual acceptance is documented in `docs/RUNBOOK-acceptance.md`.

## Documentation Rules

- Keep durable architecture in `docs/ARCHITECTURE.md`.
- Keep command/run guidance in `docs/DEVELOPMENT.md`.
- Keep current checklist state in `docs/STATUS.md`.
- Keep real hardware acceptance in `docs/RUNBOOK-acceptance.md`.
- Do not reintroduce active instructions under `docs/superpowers/`.
- Do not document stale compatibility or migration paths for API keys.

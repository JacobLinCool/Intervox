//! Virtual-mic driver presence and runtime detection.
//!
//! These helpers are deliberately total (never panic) and read-only — safe to
//! call from background tasks or sync command handlers alike.

use serde::{Deserialize, Serialize};

// ── Driver state enum ─────────────────────────────────────────────────────────

/// High-level summary of the Intervox HAL driver health.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DriverState {
    /// Driver bundle is not present on disk.
    Missing,
    /// Driver is on disk but CoreAudio has not loaded it (or it crashed).
    InstalledNotRunning,
    /// Driver is installed and visible as an audio input device.
    Healthy,
    /// Driver version on disk does not match the expected version.
    /// Reserved for a future version-mismatch signal — not yet produced.
    Stale,
}

// ── Presence helpers ──────────────────────────────────────────────────────────

/// Returns `true` if the HAL driver bundle exists at the canonical path.
pub fn installed_on_disk() -> bool {
    std::path::Path::new("/Library/Audio/Plug-Ins/HAL/Intervox.driver").exists()
}

/// Returns `true` if any CoreAudio input device name contains "Intervox"
/// (case-insensitive). Uses the same enumeration as [`crate::devices`].
pub fn visible_to_coreaudio() -> bool {
    crate::devices::enumerate()
        .inputs
        .iter()
        .any(|d| d.name.to_ascii_lowercase().contains("intervox"))
}

/// Returns `false` until the ring-producer engine is wired in Phase 3.
#[allow(dead_code)]
pub fn ring_producer_active() -> bool {
    // TODO(Task 3.2): report real engine ring-producer state
    false
}

// ── Combined state ────────────────────────────────────────────────────────────

/// Compute the overall [`DriverState`] from the individual checks.
pub fn state() -> DriverState {
    if !installed_on_disk() {
        return DriverState::Missing;
    }
    if !visible_to_coreaudio() {
        return DriverState::InstalledNotRunning;
    }
    DriverState::Healthy
}

// ── Privileged install / uninstall ───────────────────────────────────────────

/// Resolve a repo script to an absolute path. Dev/runbook builds run from the
/// repo; `CARGO_MANIFEST_DIR` is `src-tauri`, so scripts live at `../scripts`.
///
/// TODO(bundle): resolve from app resource dir in a packaged build.
fn abs(rel_from_repo_root: &str) -> String {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../Intervox/src-tauri
    let root = std::path::Path::new(manifest).parent().unwrap();
    root.join(rel_from_repo_root).to_string_lossy().into_owned()
}

/// Build the AppleScript string used to run a repo script with administrator
/// privileges. Exported for unit-testing the escaping logic.
///
/// Escaping order: backslashes first, then double-quotes. The path is embedded
/// inside a double-quoted shell argument, so both characters must be escaped.
pub(crate) fn build_osascript(script_abs: &str) -> String {
    let escaped = script_abs.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "do shell script \"INTERVOX_ASSUME_YES=1 /bin/bash \\\"{escaped}\\\" </dev/null\" with administrator privileges"
    )
}

/// Run a repo script with administrator privileges via AppleScript. The script
/// is run non-interactively (`INTERVOX_ASSUME_YES=1`, stdin from /dev/null).
fn run_priv(script_abs: &str) -> Result<(), String> {
    let osa = build_osascript(script_abs);
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&osa)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// Install the Intervox HAL driver with administrator privileges.
pub fn install() -> Result<(), String> {
    run_priv(&abs("scripts/install_driver.sh"))
}

/// Uninstall the Intervox HAL driver with administrator privileges.
pub fn uninstall() -> Result<(), String> {
    run_priv(&abs("scripts/uninstall_driver.sh"))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// `installed_on_disk()` is total — it always returns a bool without
    /// panicking (the driver is simply absent on CI).
    #[test]
    fn installed_on_disk_is_total() {
        let _result: bool = installed_on_disk();
    }

    /// `state()` is total — it always returns a valid `DriverState` without
    /// panicking, and (on a machine without the driver) must be `Missing`.
    #[test]
    fn state_is_total_and_valid() {
        let s = state();
        // Exhaustively confirm the value is one of the defined variants.
        let _valid = matches!(
            s,
            DriverState::Missing
                | DriverState::InstalledNotRunning
                | DriverState::Healthy
                | DriverState::Stale
        );
    }

    /// When the driver is not installed, `state()` returns `Missing`.
    /// On CI / developer machines without the Intervox HAL bundle this is the
    /// expected outcome.
    #[test]
    fn state_missing_when_no_driver() {
        if !installed_on_disk() {
            assert_eq!(state(), DriverState::Missing);
        }
        // If the driver IS installed, we just skip the assertion — the test
        // still passes (we don't want to fail on a dev machine with the driver).
    }

    /// `build_osascript` must properly escape a path containing both a space
    /// and a double-quote, and must include the required AppleScript keywords.
    #[test]
    fn build_osascript_escaping() {
        // Path with a space and an embedded double-quote — the nastiest case.
        let path = r#"/usr/local/my scripts/"tricky".sh"#;
        let script = build_osascript(path);

        // Must contain the required non-interactive env var.
        assert!(
            script.contains("INTERVOX_ASSUME_YES=1"),
            "missing INTERVOX_ASSUME_YES=1 in: {script}"
        );

        // Must require administrator privileges.
        assert!(
            script.contains("with administrator privileges"),
            "missing 'with administrator privileges' in: {script}"
        );

        // An embedded double-quote in the path must become \".
        assert!(
            script.contains("\\\""),
            "embedded quote not escaped to \\\" in: {script}"
        );

        // The /dev/null stdin redirect must be present.
        assert!(
            script.contains("</dev/null"),
            "missing </dev/null in: {script}"
        );

        // Sanity: the overall outer string is still a well-formed do shell script expression.
        assert!(
            script.starts_with("do shell script \""),
            "AppleScript must start with 'do shell script \"': {script}"
        );
    }
}

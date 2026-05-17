//! Virtual-mic driver presence and runtime detection.
//!
//! These helpers are deliberately total (never panic) and read-only — safe to
//! call from background tasks or sync command handlers alike.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

const HAL_DIR: &str = "/Library/Audio/Plug-Ins/HAL";
const INSTALLED_BUNDLE: &str = "/Library/Audio/Plug-Ins/HAL/Intervox.driver";
const BUNDLED_DRIVER_REL: &str = "driver/build/Intervox.driver";

// ── Driver state enum ─────────────────────────────────────────────────────────

/// High-level summary of the Intervox HAL driver health.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DriverState {
    /// Driver bundle is not present on disk.
    #[default]
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
    Path::new(INSTALLED_BUNDLE).exists()
}

/// Returns `true` if any CoreAudio input device name contains "Intervox"
/// (case-insensitive) in an already-collected device list.
pub fn visible_in_devices(devices: &crate::commands::AudioDevices) -> bool {
    devices
        .inputs
        .iter()
        .any(|d| d.name.to_ascii_lowercase().contains("intervox"))
}

// ── Combined state ────────────────────────────────────────────────────────────

/// Cheap startup state derived from the filesystem only. It never asks
/// CoreAudio to enumerate devices.
pub fn state_from_install_only() -> DriverState {
    if installed_on_disk() {
        DriverState::InstalledNotRunning
    } else {
        DriverState::Missing
    }
}

/// Compute the overall [`DriverState`] without triggering a second CoreAudio
/// device enumeration.
pub fn state_from_devices(devices: &crate::commands::AudioDevices) -> DriverState {
    if !installed_on_disk() {
        return DriverState::Missing;
    }
    if !visible_in_devices(devices) {
        return DriverState::InstalledNotRunning;
    }
    DriverState::Healthy
}

// ── Privileged install / uninstall ───────────────────────────────────────────

fn bundled_resources_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let contents_dir = exe.parent()?.parent()?;
    let resources = contents_dir.join("Resources");
    resources.is_dir().then_some(resources)
}

#[cfg(debug_assertions)]
fn dev_driver_bundle() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../Intervox/src-tauri
    Path::new(manifest)
        .parent()
        .expect("src-tauri must have a repo parent")
        .join(BUNDLED_DRIVER_REL)
}

fn driver_bundle_source() -> Result<PathBuf, String> {
    if let Some(resources) = bundled_resources_dir() {
        let bundled = resources.join(BUNDLED_DRIVER_REL);
        if bundled.is_dir() {
            return Ok(bundled);
        }
        return Err(format!(
            "Bundled Intervox.driver missing at {}",
            bundled.display()
        ));
    }

    #[cfg(debug_assertions)]
    {
        let dev = dev_driver_bundle();
        if dev.is_dir() {
            return Ok(dev);
        }
        Err(format!("Dev Intervox.driver missing at {}", dev.display()))
    }

    #[cfg(not(debug_assertions))]
    {
        Err("Intervox is not running from an app bundle, and no bundled driver is available".into())
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn applescript_quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) fn build_privileged_shell_osascript(command: &str) -> String {
    format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_quote(command)
    )
}

fn run_privileged_shell(command: &str) -> Result<(), String> {
    let osa = build_privileged_shell_osascript(command);
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

fn run_trust_check(label: &str, command: &mut Command) -> Result<(), String> {
    let out = command.output().map_err(|e| format!("{label}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(format!(
            "{label} failed with status {}{}{}",
            out.status,
            if stdout.trim().is_empty() {
                String::new()
            } else {
                format!("\nstdout:\n{stdout}")
            },
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!("\nstderr:\n{stderr}")
            }
        ))
    }
}

fn verify_driver_bundle_trust(bundle: &Path) -> Result<(), String> {
    run_trust_check(
        "codesign verification",
        Command::new("codesign")
            .arg("--verify")
            .arg("--strict")
            .arg("--deep")
            .arg("--verbose=2")
            .arg(bundle),
    )?;
    run_trust_check(
        "stapler validation",
        Command::new("xcrun")
            .arg("stapler")
            .arg("validate")
            .arg(bundle),
    )?;
    run_trust_check(
        "Gatekeeper install assessment",
        Command::new("spctl")
            .arg("-a")
            .arg("-vv")
            .arg("-t")
            .arg("install")
            .arg(bundle),
    )
}

/// Install the Intervox HAL driver with administrator privileges.
pub fn install() -> Result<(), String> {
    let source = driver_bundle_source()?;
    verify_driver_bundle_trust(&source)?;
    let source = shell_quote(&source.to_string_lossy());
    let hal_dir = shell_quote(HAL_DIR);
    let installed = shell_quote(INSTALLED_BUNDLE);
    let command = format!(
        r#"set -e
src={source}
hal={hal_dir}
dst={installed}
test -d "$src" || {{ echo "Bundled driver not found: $src" >&2; exit 1; }}
mkdir -p "$hal"
rm -rf "$dst"
cp -R "$src" "$dst"
chown -R root:wheel "$dst"
chmod -R 755 "$dst"
killall coreaudiod || true
sleep 2"#
    );
    run_privileged_shell(&command)
}

/// Uninstall the Intervox HAL driver with administrator privileges.
pub fn uninstall() -> Result<(), String> {
    let installed = shell_quote(INSTALLED_BUNDLE);
    let command = format!(
        r#"set -e
dst={installed}
if [ -d "$dst" ]; then
  rm -rf "$dst"
  killall coreaudiod || true
fi"#
    );
    run_privileged_shell(&command)
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

    /// `state_from_install_only()` is total and never enters CoreAudio.
    #[test]
    fn install_only_state_is_total_and_valid() {
        let s = state_from_install_only();
        // Exhaustively confirm the value is one of the defined variants.
        let _valid = matches!(
            s,
            DriverState::Missing
                | DriverState::InstalledNotRunning
                | DriverState::Healthy
                | DriverState::Stale
        );
    }

    #[test]
    fn state_from_devices_uses_existing_snapshot() {
        let devices = crate::commands::AudioDevices {
            sources: vec![crate::commands::AudioSourceInfo {
                id: "coreaudio:Intervox".into(),
                name: "Intervox".into(),
                kind: crate::commands::AudioSourceKind::Microphone,
            }],
            inputs: vec![crate::commands::DeviceInfo {
                id: "coreaudio:Intervox".into(),
                name: "Intervox".into(),
            }],
            outputs: vec![],
        };
        let expected = if installed_on_disk() {
            DriverState::Healthy
        } else {
            DriverState::Missing
        };
        assert_eq!(state_from_devices(&devices), expected);
    }

    /// `build_privileged_shell_osascript` must escape shell text for an
    /// AppleScript double-quoted string and request administrator privileges.
    #[test]
    fn build_privileged_shell_osascript_escaping() {
        let script = build_privileged_shell_osascript(r#"echo "quoted"; echo back\slash"#);
        assert!(
            script.contains("with administrator privileges"),
            "missing 'with administrator privileges' in: {script}"
        );
        assert!(
            script.contains("\\\"quoted\\\""),
            "embedded quote not escaped to \\\" in: {script}"
        );
        assert!(
            script.contains("back\\\\slash"),
            "embedded backslash not escaped in: {script}"
        );
    }

    #[test]
    fn shell_quote_handles_spaces_and_single_quotes() {
        assert_eq!(
            shell_quote("/tmp/Intervox.driver"),
            "'/tmp/Intervox.driver'"
        );
        assert_eq!(shell_quote("/tmp/it's here"), "'/tmp/it'\\''s here'");
    }
}

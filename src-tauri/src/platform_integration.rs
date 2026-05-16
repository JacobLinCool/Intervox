//! macOS integration for UiConfig: Dock activation policy, Login Item
//! (LaunchAgent plist), and the menu-bar latency badge text.

use intervox_core::config::UiConfig;
use std::path::PathBuf;

fn launch_agent_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join("Library/LaunchAgents/app.intervox.desktop.plist")
}

fn write_login_item(enable: bool) {
    let p = launch_agent_path();
    if !enable {
        let _ = std::fs::remove_file(&p);
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    // Prefer launching the .app bundle via LaunchServices so the bundle/
    // NSApplication lifecycle (dock, activation policy) initializes correctly.
    let app_bundle = exe
        .ancestors()
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("app"));
    let program_args = match app_bundle {
        Some(b) => format!(
            "    <string>/usr/bin/open</string>\n    <string>-a</string>\n    <string>{}</string>",
            b.display()
        ),
        None => format!("    <string>{}</string>", exe.display()),
    };
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>app.intervox.desktop</string>
  <key>ProgramArguments</key><array>
{program_args}
  </array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#
    );
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&p, plist);
}

/// Apply all UiConfig-driven OS integrations. Safe to call repeatedly.
pub fn apply_ui_config(app: &tauri::AppHandle, ui: &UiConfig) {
    #[cfg(target_os = "macos")]
    {
        let policy = if ui.hide_dock_icon {
            tauri::ActivationPolicy::Accessory
        } else {
            tauri::ActivationPolicy::Regular
        };
        let _ = app.set_activation_policy(policy);
        write_login_item(ui.launch_at_login);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        let _ = ui;
    }
}

/// Tray title: base mode label plus an optional latency badge.
pub fn tray_title(base: &str, show_badge: bool, latency_ms: Option<u32>) -> String {
    match (show_badge, latency_ms) {
        (true, Some(ms)) => format!("{base} · {:.1}s", ms as f32 / 1000.0),
        _ => base.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_title_appends_badge_only_when_enabled_and_present() {
        assert_eq!(tray_title("Translate", false, Some(1200)), "Translate");
        assert_eq!(tray_title("Translate", true, None), "Translate");
        assert_eq!(tray_title("Translate", true, Some(1180)), "Translate · 1.2s");
    }
}

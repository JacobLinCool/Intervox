//! Global shortcut registration and the pure `normalize_accelerator` helper.
//!
//! # Accelerator grammar (global-hotkey-0.7 / tauri-plugin-global-shortcut-2.3)
//!
//! The underlying parser splits on `+`, strips whitespace, and uppercases each
//! token.  Recognised modifier tokens: `OPTION`/`ALT`, `CONTROL`/`CTRL`,
//! `COMMAND`/`CMD`/`SUPER`, `SHIFT`, and cross-platform `CMDORCTRL` (etc.).
//! Everything else is treated as the main key.
//!
//! Our config stores strings like `"Cmd+Shift+T"` — these are already valid.
//! `normalize_accelerator` validates + normalises (trims, canonical casing) so
//! that registration failures are caught early and reported cleanly.

use intervox_core::state::VirtualMicMode;
use tauri::Emitter as _;
use tauri_plugin_global_shortcut::GlobalShortcutExt as _;

// ── Pure normalizer ───────────────────────────────────────────────────────────

/// Normalise a human-entered accelerator string into the canonical form
/// accepted by `tauri-plugin-global-shortcut` / `global-hotkey`.
///
/// Rules:
/// - Split on `+`.
/// - Trim and validate each token (must be non-empty, must map to a known
///   modifier or a recognised key).
/// - Re-join with `+` using canonical casing (`Shift`, `Cmd`, `Ctrl`, `Alt`,
///   `Super`, `CmdOrCtrl`, title-case key).
/// - Return `None` if the string is empty, contains an unparseable token, or
///   has no non-modifier key.
///
/// This is a **pure** function — no I/O, no Tauri runtime.
pub fn normalize_accelerator(raw: &str) -> Option<String> {
    if raw.trim().is_empty() {
        return None;
    }

    // We validate by attempting to parse via the plugin's own parser, but we
    // also re-build a canonical string ourselves so callers get a stable form.
    let tokens: Vec<&str> = raw.split('+').collect();
    if tokens.is_empty() {
        return None;
    }

    let mut mods: Vec<&str> = Vec::new();
    let mut main_key: Option<String> = None;

    for raw_token in &tokens {
        let token = raw_token.trim();
        if token.is_empty() {
            return None; // "++" or trailing "+"
        }
        match token.to_uppercase().as_str() {
            "OPTION" | "ALT" => mods.push("Alt"),
            "CONTROL" | "CTRL" => mods.push("Ctrl"),
            "COMMAND" | "CMD" | "SUPER" => mods.push("Cmd"),
            "SHIFT" => mods.push("Shift"),
            "COMMANDORCONTROL" | "COMMANDORCTRL" | "CMDORCTRL" | "CMDORCONTROL" => {
                mods.push("CmdOrCtrl")
            }
            _ => {
                if main_key.is_some() {
                    // Two non-modifier tokens → invalid.
                    return None;
                }
                // Validate the key token by trying parse.  We do this by
                // building a minimal accelerator string and asking the plugin
                // to parse it; if it fails the token is garbage.
                let probe = format!("Shift+{token}");
                if probe.parse::<tauri_plugin_global_shortcut::Shortcut>().is_err() {
                    return None;
                }
                // Store the token in the casing the parser accepts (it
                // upper-cases internally, so any casing works; we normalise to
                // title-case for readability, which the parser also accepts).
                main_key = Some(title_case(token));
            }
        }
    }

    let key = main_key?; // no main key → invalid
    // Deduplicate modifiers while preserving the first occurrence order.
    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<&str> = mods
        .iter()
        .filter(|&&m| seen.insert(m))
        .copied()
        .collect();

    if deduped.is_empty() {
        // A shortcut with no modifiers is accepted by the parser but is
        // unsafe as a global shortcut.  Allow it — the caller decides.
        Some(key)
    } else {
        Some(format!("{}+{}", deduped.join("+"), key))
    }
}

/// Return `s` in Title Case (first char upper, rest lower).  Used so that
/// single-letter keys normalise to e.g. `T`, `M`, `C` which the parser
/// accepts (it uppercases internally, so any case works).
fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
        }
    }
}

// ── Registration ──────────────────────────────────────────────────────────────

/// (Re)register all configured global shortcuts.
///
/// 1. Unregisters every previously-registered shortcut.
/// 2. Reads the current shortcuts config.
/// 3. Normalises each accelerator.
/// 4. Registers each with its action handler.
///
/// On any per-shortcut failure, emits an `AppError` event with an
/// accessibility-settings recovery action and continues registering the rest
/// (partial registration is acceptable).
pub fn register_shortcuts(app: &tauri::AppHandle) {
    use tauri::Manager as _;

    // Unregister all previously registered shortcuts.  Best-effort.
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();

    // Read shortcuts config — clone immediately to drop the mutex guard.
    let h = app.state::<crate::commands::AppHandle>();
    let shortcuts_cfg = h.config.lock().unwrap().shortcuts.clone();

    // Helper closure: register one shortcut with an action.
    let try_register = |raw: &str, action: ShortcutAction| {
        let normalised = match normalize_accelerator(raw) {
            Some(n) => n,
            None => {
                emit_shortcut_error(app, raw, "Accelerator string is not parseable");
                return;
            }
        };

        let app_clone = app.clone();
        let result = app.global_shortcut().on_shortcut(
            normalised.as_str(),
            move |app, _shortcut, event| {
                if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                    execute_shortcut_action(app, action);
                }
                let _ = app_clone; // keep lifetime
            },
        );

        if let Err(e) = result {
            emit_shortcut_error(app, raw, &e.to_string());
        }
    };

    // toggle_translate
    try_register(&shortcuts_cfg.toggle_translate, ShortcutAction::ToggleTranslate);
    // silence
    try_register(&shortcuts_cfg.silence, ShortcutAction::Silence);
    // captions
    try_register(&shortcuts_cfg.captions, ShortcutAction::Captions);
}

/// Which action a global shortcut fires.
#[derive(Clone, Copy)]
enum ShortcutAction {
    ToggleTranslate,
    Silence,
    Captions,
}

fn execute_shortcut_action(app: &tauri::AppHandle, action: ShortcutAction) {
    use tauri::Manager as _;
    let h = app.state::<crate::commands::AppHandle>();
    let engine = app.state::<std::sync::Arc<crate::engine::Engine>>();

    match action {
        ShortcutAction::ToggleTranslate => {
            let current_mode = h.state.lock().unwrap().status.mode;
            let new_mode = if matches!(
                current_mode,
                VirtualMicMode::Translate | VirtualMicMode::TranslateWithOriginal
            ) {
                VirtualMicMode::Silence
            } else {
                VirtualMicMode::Translate
            };
            crate::commands::apply_mode(app, &h, &engine, new_mode);
        }
        ShortcutAction::Silence => {
            crate::commands::apply_mode(app, &h, &engine, VirtualMicMode::Silence);
        }
        ShortcutAction::Captions => {
            if app.get_webview_window("captions").is_some() {
                let _ = crate::commands::do_close_captions_window(app);
            } else {
                let always_on_top = h.config.lock().unwrap().captions.always_on_top;
                let _ = crate::commands::do_open_captions_window(app, always_on_top);
            }
        }
    }
}

fn emit_shortcut_error(app: &tauri::AppHandle, accel: &str, reason: &str) {
    use intervox_core::{AppError, AppErrorCode, RecoveryAction};

    let err = AppError::new(
        AppErrorCode::Internal,
        "Shortcut registration failed",
        format!(
            "Could not register shortcut \"{accel}\": {reason}. \
             On macOS, global shortcuts do not require Accessibility permission, \
             but if the key combo is already claimed by another app, \
             try a different combination."
        ),
        Some(RecoveryAction {
            label: "Open Accessibility Settings".into(),
            command: "open_accessibility_settings".into(),
        }),
    );

    let _ = app.emit("error", err);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::normalize_accelerator;

    // ── Valid combos ──────────────────────────────────────────────────────────

    #[test]
    fn default_toggle_translate_normalizes() {
        // Config default: "Cmd+Shift+T"
        let result = normalize_accelerator("Cmd+Shift+T");
        assert!(result.is_some(), "expected Some, got None");
        let s = result.unwrap();
        // Must contain Cmd, Shift, and T fragments
        assert!(s.contains("Cmd"), "missing Cmd in '{s}'");
        assert!(s.contains("Shift"), "missing Shift in '{s}'");
        assert!(s.to_uppercase().contains('T'), "missing T in '{s}'");
    }

    #[test]
    fn default_silence_normalizes() {
        // Config default: "Cmd+Shift+M"
        let result = normalize_accelerator("Cmd+Shift+M");
        assert!(result.is_some());
        let s = result.unwrap();
        assert!(s.contains("Cmd"));
        assert!(s.contains("Shift"));
        assert!(s.to_uppercase().contains('M'));
    }

    #[test]
    fn default_captions_normalizes() {
        // Config default: "Cmd+Shift+C"
        let result = normalize_accelerator("Cmd+Shift+C");
        assert!(result.is_some());
        let s = result.unwrap();
        assert!(s.contains("Cmd"));
        assert!(s.contains("Shift"));
        assert!(s.to_uppercase().contains('C'));
    }

    // ── Invalid inputs → None ─────────────────────────────────────────────────

    #[test]
    fn empty_string_returns_none() {
        assert_eq!(normalize_accelerator(""), None);
    }

    #[test]
    fn whitespace_only_returns_none() {
        assert_eq!(normalize_accelerator("   "), None);
    }

    #[test]
    fn garbage_key_returns_none() {
        assert_eq!(normalize_accelerator("Cmd+Shift+NotAKey!!!"), None);
    }

    #[test]
    fn double_plus_returns_none() {
        assert_eq!(normalize_accelerator("Cmd++T"), None);
    }

    #[test]
    fn modifiers_only_returns_none() {
        // No main key
        assert_eq!(normalize_accelerator("Cmd+Shift"), None);
    }

    #[test]
    fn two_main_keys_returns_none() {
        assert_eq!(normalize_accelerator("Cmd+T+M"), None);
    }

    // ── Case-insensitive modifiers ────────────────────────────────────────────

    #[test]
    fn lowercase_modifiers_normalize() {
        let r = normalize_accelerator("cmd+shift+t");
        assert!(r.is_some(), "lowercase modifiers should be accepted");
    }

    #[test]
    fn uppercase_modifiers_normalize() {
        let r = normalize_accelerator("CMD+SHIFT+T");
        assert!(r.is_some(), "uppercase modifiers should be accepted");
    }

    #[test]
    fn mixed_case_modifiers_normalize() {
        let r = normalize_accelerator("CmD+ShIfT+t");
        assert!(r.is_some(), "mixed-case modifiers should be accepted");
    }

    // ── Alias modifiers ───────────────────────────────────────────────────────

    #[test]
    fn command_alias_normalizes() {
        let r = normalize_accelerator("Command+Shift+T");
        assert!(r.is_some());
    }

    #[test]
    fn super_alias_normalizes() {
        let r = normalize_accelerator("Super+Shift+T");
        assert!(r.is_some());
    }

    #[test]
    fn cmdorctrl_alias_normalizes() {
        let r = normalize_accelerator("CmdOrCtrl+Shift+T");
        assert!(r.is_some());
    }

    #[test]
    fn ctrl_normalizes() {
        let r = normalize_accelerator("Ctrl+Shift+T");
        assert!(r.is_some());
    }

    #[test]
    fn alt_normalizes() {
        let r = normalize_accelerator("Alt+T");
        assert!(r.is_some());
    }

    // ── Canonical form ────────────────────────────────────────────────────────

    #[test]
    fn canonical_form_is_parseable_by_plugin() {
        // Verify the normalised string can be parsed by the plugin's own Shortcut type.
        for raw in &["Cmd+Shift+T", "Cmd+Shift+M", "Cmd+Shift+C", "CmdOrCtrl+T"] {
            let normalised = normalize_accelerator(raw).expect("normalise");
            let parsed = normalised.parse::<tauri_plugin_global_shortcut::Shortcut>();
            assert!(
                parsed.is_ok(),
                "normalised '{normalised}' (from '{raw}') failed to parse: {:?}",
                parsed
            );
        }
    }

    #[test]
    fn raw_defaults_are_directly_parseable_by_plugin() {
        // The raw config defaults must also parse without normalisation.
        for raw in &["Cmd+Shift+T", "Cmd+Shift+M", "Cmd+Shift+C"] {
            let parsed = raw.parse::<tauri_plugin_global_shortcut::Shortcut>();
            assert!(parsed.is_ok(), "raw '{raw}' failed to parse: {:?}", parsed);
        }
    }
}

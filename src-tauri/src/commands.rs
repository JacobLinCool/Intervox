//! Tauri command surface (spec §4.1). The frontend never touches audio or
//! OpenAI directly — every native operation goes through these. Device
//! enumeration, config/secret persistence and key validation are real; the
//! live audio engine and websocket transport are wired in later phases.

use intervox_core::audio::mixer::MixSettings;
use intervox_core::config::{CaptionsConfig, PrivacyConfig, ShortcutsConfig};
use intervox_core::state::{AppState, AppStatus, VirtualMicMode};
use intervox_core::{AppError, Config};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Default)]
pub struct AppHandle {
    pub state: Mutex<AppState>,
    pub config: Mutex<Config>,
}

impl AppHandle {
    /// Build an `AppHandle` hydrated from the persisted config on disk.
    /// Falls back to defaults on any error.
    pub fn hydrated() -> Self {
        let cfg = crate::appcfg::load_or_default();
        let mut state = AppState::new();
        if let Ok(mode) = serde_json::from_value::<VirtualMicMode>(
            serde_json::Value::String(cfg.audio.virtual_mic_mode.clone()),
        ) {
            state.transition(mode);
        }
        AppHandle {
            state: Mutex::new(state),
            config: Mutex::new(cfg),
        }
    }
}

/// Persist the current config to disk. Must be called **after** dropping any
/// `MutexGuard` on `h.config` to avoid a deadlock.
fn save_config(h: &AppHandle) {
    crate::appcfg::persist(&h.config.lock().unwrap());
}

// ── Tray helpers (pure, no Tauri runtime) ────────────────────────────────────

/// Short label shown in the tray title / tooltip for each mode.
/// Order: [Silence, PassThrough, Translate, TranslateWithOriginal].
pub fn tray_mode_label(mode: VirtualMicMode) -> &'static str {
    match mode {
        VirtualMicMode::Silence => "Silence",
        VirtualMicMode::PassThrough => "Pass-Through",
        VirtualMicMode::Translate => "Translate",
        VirtualMicMode::TranslateWithOriginal => "Translate+Orig",
    }
}

/// Returns `[silence_checked, passthrough_checked, translate_checked, translate_orig_checked]`.
pub fn tray_menu_checks(current: VirtualMicMode) -> [bool; 4] {
    [
        current == VirtualMicMode::Silence,
        current == VirtualMicMode::PassThrough,
        current == VirtualMicMode::Translate,
        current == VirtualMicMode::TranslateWithOriginal,
    ]
}

// ── DRY mode-application helper ───────────────────────────────────────────────

/// Apply a mode change: update AppState, persist config, drive engine, emit
/// `status-changed`, then refresh the tray title and checked-state.
///
/// Takes plain refs (NOT `tauri::State`) so it can be called from both the
/// `#[tauri::command]` (which resolves its `tauri::State` params first) and
/// the tray `on_menu_event` closure (which resolves state via `app.state()`).
///
/// Deadlock discipline: every `MutexGuard` is dropped before `save_config` and
/// before `app.emit(...)`, matching the original command body.
pub fn apply_mode(
    app: &tauri::AppHandle,
    h: &AppHandle,
    engine: &std::sync::Arc<crate::engine::Engine>,
    mode: VirtualMicMode,
) {
    // 1. Transition state (guard dropped immediately).
    h.state.lock().unwrap().transition(mode);

    // 2. Write mode string to config (guard dropped immediately).
    h.config.lock().unwrap().audio.virtual_mic_mode =
        serde_json::to_value(mode).unwrap().as_str().unwrap().to_string();

    // 3. Persist — no guards held.
    save_config(h);

    // 4. Drive engine.
    engine.set_mode(mode);

    // 5. Emit status — read status with a fresh lock, then drop before emit.
    let status = h.state.lock().unwrap().status.clone();
    {
        use tauri::Emitter;
        let _ = app.emit("status-changed", status);
    }

    // 6. Update tray title + checked-state (best-effort; errors ignored).
    use tauri::Manager as _;
    if let Some(tray_state) = app.try_state::<TrayState>() {
        let label = tray_mode_label(mode);
        let _ = tray_state.tray.set_title(Some(label));
        let checks = tray_menu_checks(mode);
        let _ = tray_state.mode_silence.set_checked(checks[0]);
        let _ = tray_state.mode_passthrough.set_checked(checks[1]);
        let _ = tray_state.mode_translate.set_checked(checks[2]);
        let _ = tray_state.mode_translate_orig.set_checked(checks[3]);
    }
}

// ── Tray managed state ─────────────────────────────────────────────────────────

/// Holds the `TrayIcon` handle plus the 4 `CheckMenuItem` handles so that
/// `apply_mode` can update their checked-state and the tray title without
/// rebuilding the whole menu. `TrayIcon<Wry>` is `Send + Sync` (Tauri marks
/// it so); `CheckMenuItem<Wry>` likewise. Stored as Tauri managed state.
pub struct TrayState {
    pub tray: tauri::tray::TrayIcon,
    pub mode_silence: tauri::menu::CheckMenuItem<tauri::Wry>,
    pub mode_passthrough: tauri::menu::CheckMenuItem<tauri::Wry>,
    pub mode_translate: tauri::menu::CheckMenuItem<tauri::Wry>,
    pub mode_translate_orig: tauri::menu::CheckMenuItem<tauri::Wry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevices {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

#[tauri::command]
pub fn get_app_status(h: tauri::State<AppHandle>) -> Result<AppStatus, AppError> {
    let installed =
        crate::driver_status::installed_on_disk() && crate::driver_status::visible_to_coreaudio();
    let mut st = h.state.lock().unwrap();
    st.status.virtual_mic_installed = installed;
    Ok(st.status.clone())
}

#[tauri::command]
pub fn set_virtual_mic_mode(
    mode: VirtualMicMode,
    app: tauri::AppHandle,
    h: tauri::State<'_, AppHandle>,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    apply_mode(&app, &h, &engine, mode);
    Ok(())
}

#[tauri::command]
pub fn get_audio_devices() -> Result<AudioDevices, AppError> {
    Ok(crate::devices::enumerate())
}

#[tauri::command]
pub fn set_source_mic(
    device_id: String,
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    h.config.lock().unwrap().audio.source_mic_id = Some(device_id.clone());
    h.state.lock().unwrap().status.source_mic_name = Some(device_id.clone());
    save_config(&h);
    engine.set_source_device(device_id);
    {
        use tauri::Emitter;
        let _ = app.emit("status-changed", h.state.lock().unwrap().status.clone());
    }
    Ok(())
}

#[tauri::command]
pub fn set_monitor_output(
    device_id: Option<String>,
    h: tauri::State<AppHandle>,
) -> Result<(), AppError> {
    h.config.lock().unwrap().audio.monitor_output_id = device_id;
    save_config(&h);
    Ok(())
}

#[tauri::command]
pub fn set_target_language(
    language: String,
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    // Clone what we need before dropping the guards.
    let (src, tgt) = {
        let mut cfg = h.config.lock().unwrap();
        cfg.translation.target_language = language.clone();
        let src = cfg.translation.source_language.clone();
        let tgt = language.clone();
        (src, tgt)
    }; // config MutexGuard dropped
    {
        h.state.lock().unwrap().status.target_language = language;
    } // state MutexGuard dropped
    save_config(&h);
    // Drive the engine — only restarts an active OpenAI session; no-op in Silence/PassThrough.
    engine.set_languages(src, tgt);
    {
        use tauri::Emitter;
        let _ = app.emit("status-changed", h.state.lock().unwrap().status.clone());
    }
    Ok(())
}

#[tauri::command]
pub fn set_mix_settings(
    settings: MixSettings,
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
) -> Result<(), AppError> {
    let mut cfg = h.config.lock().unwrap();
    cfg.mix.duck_original = settings.duck_original;
    cfg.audio.limiter_enabled = settings.limiter_enabled;
    drop(cfg);
    save_config(&h);
    {
        use tauri::Emitter;
        let _ = app.emit("status-changed", h.state.lock().unwrap().status.clone());
    }
    Ok(())
}

#[tauri::command]
pub fn install_virtual_mic() -> Result<(), AppError> {
    crate::driver_status::install().map_err(AppError::internal)?;
    // Re-check driver state after the privileged install.
    match crate::driver_status::state() {
        crate::driver_status::DriverState::Healthy => Ok(()),
        _ => Err(AppError::driver_missing()),
    }
}

#[tauri::command]
pub fn update_virtual_mic() -> Result<(), AppError> {
    // "Update" == reinstall the bundled driver (same script, privileged).
    crate::driver_status::install().map_err(AppError::internal)?;
    match crate::driver_status::state() {
        crate::driver_status::DriverState::Healthy => Ok(()),
        _ => Err(AppError::driver_missing()),
    }
}

#[tauri::command]
pub fn uninstall_virtual_mic() -> Result<(), AppError> {
    crate::driver_status::uninstall().map_err(AppError::internal)
}

#[tauri::command]
pub fn get_driver_state() -> Result<crate::driver_status::DriverState, AppError> {
    Ok(crate::driver_status::state())
}

#[tauri::command]
pub fn open_audio_midi_setup() -> Result<(), AppError> {
    std::process::Command::new("open")
        .arg("-a")
        .arg("Audio MIDI Setup")
        .spawn()
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn open_system_mic_permission_settings() -> Result<(), AppError> {
    crate::permission::open_privacy_pane();
    Ok(())
}

#[tauri::command]
pub fn get_mic_permission() -> Result<crate::permission::MicPermission, AppError> {
    Ok(crate::permission::status())
}

/// Generate a pure ~440 Hz sine tone at `sample_rate` Hz, `secs` seconds long,
/// peak amplitude `amp` (linear, 0.0–1.0). Used both by `start_test_phrase` and
/// by the unit test below — no I/O or shm dependency.
pub(crate) fn test_tone(sample_rate: u32, secs: f32, hz: f32, amp: f32) -> Vec<f32> {
    let n = (sample_rate as f32 * secs).round() as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            amp * (2.0 * std::f32::consts::PI * hz * t).sin()
        })
        .collect()
}

/// Write a ~1 s diagnostic tone (440 Hz sine, amplitude 0.3) directly into the
/// ring buffer so the user can confirm the virtual mic path is working.
///
/// This command does NOT change the engine mode, capture state, or OpenAI
/// session — it is purely diagnostic.  The ring has 8 s capacity (Task 3.2),
/// so a single 1 s write is safe and avoids introducing click artifacts that
/// chunked writes with sleeps could produce.
#[tauri::command]
pub fn start_test_phrase(
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    let tone = test_tone(48_000, 1.0, 440.0, 0.3);
    engine.ring().write(&tone);
    Ok(())
}

/// Add a `clear_transcript_history` command that emits a `"transcript-cleared"`
/// event.  There is no on-disk transcript history (default
/// `privacy.save_transcript_history=false`), so "clear" resets the live/in-session
/// transcript buffers held by the frontend store.  The event name is the durable
/// contract; the store subscribes and zeros `srcText`/`tgtText` on receipt.
#[tauri::command]
pub fn clear_transcript_history(app: tauri::AppHandle) -> Result<(), AppError> {
    use tauri::Emitter;
    let _ = app.emit("transcript-cleared", ());
    Ok(())
}

#[tauri::command]
pub fn stop_all_audio(
    app: tauri::AppHandle,
    h: tauri::State<'_, AppHandle>,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    apply_mode(&app, &h, &engine, VirtualMicMode::Silence);
    Ok(())
}

// ── Account / API key ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub has_key: bool,
    pub verified: bool,
    pub masked_key: Option<String>,
    pub last_verified: Option<String>,
    pub usage_usd: f64,
}

/// Mask an API key for display: keep first 7 chars + 20 bullet chars + last 4
/// for keys longer than 11 chars; otherwise return 8 bullet chars.
pub(crate) fn mask_key(k: &str) -> String {
    if k.len() > 11 {
        format!("{}{}{}", &k[..7], "\u{2022}".repeat(20), &k[k.len() - 4..])
    } else {
        "\u{2022}".repeat(8)
    }
}

fn account_status(verified: bool) -> AccountStatus {
    let key = crate::secrets::get_key();
    let masked = key.as_deref().map(mask_key);
    AccountStatus {
        has_key: key.is_some(),
        verified: key.is_some() && verified,
        masked_key: masked,
        last_verified: None,
        usage_usd: 0.0,
    }
}

#[tauri::command]
pub fn get_account_status() -> Result<AccountStatus, AppError> {
    Ok(account_status(false))
}

#[tauri::command]
pub fn set_api_key(key: String) -> Result<AccountStatus, AppError> {
    crate::secrets::set_key(&key).map_err(AppError::internal)?;
    Ok(account_status(false))
}

#[tauri::command]
pub async fn verify_api_key() -> Result<AccountStatus, AppError> {
    use intervox_core::realtime::openai_auth::{classify_validation, KeyValidation, VALIDATION_URL};

    let key = crate::secrets::get_key()
        .ok_or_else(|| AppError::openai_auth_error("No API key set"))?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| AppError::internal(e.to_string()))?;

    let http_status = match client
        .get(VALIDATION_URL)
        .bearer_auth(&key)
        .send()
        .await
    {
        Ok(resp) => Some(resp.status().as_u16()),
        Err(_) => None,
    };

    match classify_validation(http_status) {
        KeyValidation::Verified => {
            let mut s = account_status(true);
            s.last_verified = Some(rfc3339_now());
            Ok(s)
        }
        KeyValidation::InvalidKey => Err(AppError::openai_auth_error("Invalid API key")),
        KeyValidation::Offline | KeyValidation::Unknown => Ok(account_status(false)),
    }
}

/// Returns the current UTC time formatted as an RFC 3339 string,
/// e.g. `2026-05-16T12:34:56Z`. Uses only `std::time` — no extra crate dep.
fn rfc3339_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Convert Unix epoch seconds to a calendar UTC timestamp.
    let (y, mo, d, h, mi, s) = unix_secs_to_utc(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Minimal UTC calendar decomposition from Unix seconds (no leap seconds).
fn unix_secs_to_utc(mut secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    secs /= 60;
    let mi = secs % 60;
    secs /= 60;
    let h = secs % 24;
    let days = secs / 24;

    // Days since epoch (1970-01-01). Use the proleptic Gregorian algorithm.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };

    (y, mo, d, h, mi, s)
}

#[tauri::command]
pub fn clear_api_key() -> Result<(), AppError> {
    crate::secrets::clear_key().map_err(AppError::internal)
}

#[cfg(test)]
mod tests {
    use super::{mask_key, test_tone, tray_menu_checks, tray_mode_label};
    use intervox_core::state::VirtualMicMode;

    // ── tray_mode_label ───────────────────────────────────────────────────────

    #[test]
    fn tray_mode_label_all_four_distinct() {
        let labels = [
            tray_mode_label(VirtualMicMode::Silence),
            tray_mode_label(VirtualMicMode::PassThrough),
            tray_mode_label(VirtualMicMode::Translate),
            tray_mode_label(VirtualMicMode::TranslateWithOriginal),
        ];
        // All four labels must be non-empty and distinct.
        for l in &labels {
            assert!(!l.is_empty(), "label must not be empty");
        }
        let mut sorted = labels;
        sorted.sort_unstable();
        for w in sorted.windows(2) {
            assert_ne!(w[0], w[1], "labels must be distinct");
        }
    }

    #[test]
    fn tray_mode_label_stable() {
        assert_eq!(tray_mode_label(VirtualMicMode::Silence), "Silence");
        assert_eq!(tray_mode_label(VirtualMicMode::PassThrough), "Pass-Through");
        assert_eq!(tray_mode_label(VirtualMicMode::Translate), "Translate");
        assert_eq!(
            tray_mode_label(VirtualMicMode::TranslateWithOriginal),
            "Translate+Orig"
        );
    }

    // ── tray_menu_checks ──────────────────────────────────────────────────────

    #[test]
    fn tray_menu_checks_exactly_one_true_per_mode() {
        let modes = [
            VirtualMicMode::Silence,
            VirtualMicMode::PassThrough,
            VirtualMicMode::Translate,
            VirtualMicMode::TranslateWithOriginal,
        ];
        for mode in modes {
            let checks = tray_menu_checks(mode);
            let true_count = checks.iter().filter(|&&b| b).count();
            assert_eq!(
                true_count, 1,
                "exactly one check must be true for mode {:?}, got {true_count}",
                mode
            );
        }
    }

    #[test]
    fn tray_menu_checks_correct_index() {
        // Order: [Silence=0, PassThrough=1, Translate=2, TranslateWithOriginal=3]
        assert_eq!(
            tray_menu_checks(VirtualMicMode::Silence),
            [true, false, false, false]
        );
        assert_eq!(
            tray_menu_checks(VirtualMicMode::PassThrough),
            [false, true, false, false]
        );
        assert_eq!(
            tray_menu_checks(VirtualMicMode::Translate),
            [false, false, true, false]
        );
        assert_eq!(
            tray_menu_checks(VirtualMicMode::TranslateWithOriginal),
            [false, false, false, true]
        );
    }

    #[test]
    fn mask_long_key() {
        // Key longer than 11 chars: first 7 + 20 bullets + last 4.
        let key = "sk-proj-ABCDEFGHIJKLMNOPQRSTUVWXYZ1234";
        let masked = mask_key(key);
        let expected_prefix = &key[..7];
        let expected_suffix = &key[key.len() - 4..];
        let bullets = "\u{2022}".repeat(20);
        assert_eq!(masked, format!("{expected_prefix}{bullets}{expected_suffix}"));
        // Bullet count should be exactly 20.
        assert_eq!(
            masked.chars().filter(|&c| c == '\u{2022}').count(),
            20
        );
    }

    #[test]
    fn mask_short_key() {
        // Key 11 chars or fewer: 8 bullets.
        let key = "sk-short";
        let masked = mask_key(key);
        assert_eq!(masked, "\u{2022}".repeat(8));
    }

    #[test]
    fn mask_exactly_11_chars() {
        let key = "12345678901"; // exactly 11 chars
        let masked = mask_key(key);
        assert_eq!(masked, "\u{2022}".repeat(8));
    }

    #[test]
    fn mask_exactly_12_chars() {
        let key = "123456789012"; // exactly 12 chars → long path
        let masked = mask_key(key);
        let bullets = "\u{2022}".repeat(20);
        assert_eq!(masked, format!("1234567{bullets}9012"));
    }

    // ── test_tone ─────────────────────────────────────────────────────────────

    #[test]
    fn test_tone_length() {
        let tone = test_tone(48_000, 1.0, 440.0, 0.3);
        assert_eq!(tone.len(), 48_000, "1 s at 48 kHz must produce exactly 48 000 samples");
    }

    #[test]
    fn test_tone_amplitude() {
        // Peak amplitude of the returned slice must be ≈ amp (within 1 %).
        let amp = 0.3_f32;
        let tone = test_tone(48_000, 1.0, 440.0, amp);
        let peak = tone.iter().cloned().fold(0.0_f32, f32::max);
        assert!(
            (peak - amp).abs() < 0.01,
            "peak {peak} must be within 1 % of amp {amp}"
        );
    }

    #[test]
    fn test_tone_not_all_zeros() {
        let tone = test_tone(48_000, 1.0, 440.0, 0.3);
        assert!(
            tone.iter().any(|&s| s != 0.0),
            "tone must contain non-zero samples"
        );
    }
}

// ── Config commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_config(h: tauri::State<AppHandle>) -> Result<Config, AppError> {
    Ok(h.config.lock().unwrap().clone())
}

#[tauri::command]
pub fn set_source_language(
    language: String,
    h: tauri::State<AppHandle>,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    // Clone what we need before dropping the guard.
    let (src, tgt) = {
        let mut cfg = h.config.lock().unwrap();
        cfg.translation.source_language = language;
        let src = cfg.translation.source_language.clone();
        let tgt = cfg.translation.target_language.clone();
        (src, tgt)
    }; // config MutexGuard dropped
    save_config(&h);
    // Drive the engine — only restarts an active OpenAI session; no-op in Silence/PassThrough.
    engine.set_languages(src, tgt);
    Ok(())
}

#[tauri::command]
pub fn set_quality_mode(quality: String, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    h.config.lock().unwrap().translation.quality_mode = quality;
    save_config(&h);
    Ok(())
}

#[tauri::command]
pub fn set_mix_percent(percent: u32, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    let clamped = percent.min(30);
    h.config.lock().unwrap().mix.original_voice_percent = clamped;
    save_config(&h);
    Ok(())
}

#[tauri::command]
pub fn set_captions_config(c: CaptionsConfig, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    h.config.lock().unwrap().captions = c;
    save_config(&h);
    Ok(())
}

#[tauri::command]
pub fn set_privacy_config(p: PrivacyConfig, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    h.config.lock().unwrap().privacy = p;
    save_config(&h);
    Ok(())
}

#[tauri::command]
pub fn set_shortcuts(
    s: ShortcutsConfig,
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
) -> Result<(), AppError> {
    h.config.lock().unwrap().shortcuts = s;
    save_config(&h);
    // Re-register global shortcuts with the new config (drop the guard first).
    crate::shortcuts::register_shortcuts(&app);
    Ok(())
}

/// Open System Settings › Privacy & Security › Accessibility.
///
/// macOS global shortcuts (via `tauri-plugin-global-shortcut`) do NOT require
/// Accessibility permission, but this command is surfaced as a recovery action
/// in the error emitted when shortcut registration fails, so users can inspect
/// the pane if they think another app has claimed the key combo.
#[tauri::command]
pub fn open_accessibility_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

#[tauri::command]
pub fn complete_onboarding(h: tauri::State<AppHandle>) -> Result<(), AppError> {
    h.config.lock().unwrap().onboarding_completed = true;
    save_config(&h);
    Ok(())
}

// ── Captions window commands ─────────────────────────────────────────────────

/// Inner helper — open or show the captions window. Can be called from both
/// the `#[tauri::command]` and the tray `on_menu_event` closure.
///
/// If the `captions` webview window already exists, shows + refocuses it and
/// refreshes the `always_on_top` flag from `always_on_top`.  Otherwise builds
/// a new window:
///   • label "captions" → loads dist/captions.html
///   • `decorations(false)` — no OS chrome
///   • `always_on_top(always_on_top)` — from config
///   • `transparent(true)` — requires `macOSPrivateApi: true` in tauri.conf.json
///   • `skip_taskbar(true)` — not in the dock/taskbar
///   • 520 × 200 initial size, resizable
///   • immediately visible
pub fn do_open_captions_window(
    app: &tauri::AppHandle,
    always_on_top: bool,
) -> Result<(), AppError> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    if let Some(win) = app.get_webview_window("captions") {
        // Window already exists — just show, focus, and refresh always_on_top.
        let _ = win.set_always_on_top(always_on_top);
        let _ = win.show();
        let _ = win.set_focus();
    } else {
        WebviewWindowBuilder::new(app, "captions", WebviewUrl::App("captions.html".into()))
            .title("")
            .decorations(false)
            .always_on_top(always_on_top)
            .transparent(true)
            .skip_taskbar(true)
            .inner_size(520.0, 200.0)
            .resizable(true)
            .visible(true)
            .build()
            .map_err(|e| AppError::internal(format!("captions window: {e}")))?;
    }
    Ok(())
}

/// Inner helper — close the captions window. Silently succeeds if absent.
pub fn do_close_captions_window(app: &tauri::AppHandle) -> Result<(), AppError> {
    use tauri::Manager;
    if let Some(win) = app.get_webview_window("captions") {
        win.close().map_err(|e| AppError::internal(format!("close captions: {e}")))?;
    }
    Ok(())
}

/// Tauri command: open the dedicated always-on-top captions window.
#[tauri::command]
pub fn open_captions_window(
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
) -> Result<(), AppError> {
    let always_on_top = h.config.lock().unwrap().captions.always_on_top;
    do_open_captions_window(&app, always_on_top)
}

/// Tauri command: close (destroy) the dedicated captions window.
#[tauri::command]
pub fn close_captions_window(app: tauri::AppHandle) -> Result<(), AppError> {
    do_close_captions_window(&app)
}

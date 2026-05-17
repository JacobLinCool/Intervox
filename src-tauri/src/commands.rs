//! Tauri command surface (spec §4.1). The frontend never touches audio or
//! OpenAI directly — every native operation goes through these. Device
//! enumeration, config/secret persistence, key validation, live audio routing,
//! and websocket transport are all wired through this native command layer.

use intervox_core::audio::mixer::MixSettings;
use intervox_core::config::{AccountConfig, CaptionsConfig, PrivacyConfig, ShortcutsConfig};
use intervox_core::state::{AppState, AppStatus, VirtualMicMode};
use intervox_core::{AppError, Config};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use crate::driver_status::DriverState;
use crate::permission::MicPermission;

#[derive(Default)]
pub struct AppHandle {
    pub state: Mutex<AppState>,
    pub config: Mutex<Config>,
    pub driver_state: Mutex<DriverState>,
    pub audio_enumeration_running: Arc<AtomicBool>,
}

impl AppHandle {
    /// Build an `AppHandle` hydrated from the persisted config on disk.
    /// Falls back to defaults on any error.
    pub fn hydrated() -> Self {
        let cfg = crate::appcfg::load_or_default();
        let mut state = AppState::new();
        if let Ok(mode) = serde_json::from_value::<VirtualMicMode>(serde_json::Value::String(
            cfg.audio.virtual_mic_mode.clone(),
        )) {
            state.transition(mode);
        }
        state.status.source_mic_name = cfg.audio.source_mic_id.as_deref().map(device_label_from_id);
        state.status.target_language = cfg.translation.target_language.clone();
        AppHandle {
            state: Mutex::new(state),
            config: Mutex::new(cfg),
            driver_state: Mutex::new(crate::driver_status::state_from_install_only()),
            audio_enumeration_running: Arc::new(AtomicBool::new(false)),
        }
    }
}

const AUDIO_ENUMERATION_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn enumerate_audio_devices_bounded(
    running: Arc<AtomicBool>,
) -> Result<AudioDevices, AppError> {
    if running.swap(true, Ordering::AcqRel) {
        return Err(AppError::internal(
            "audio device enumeration is already in progress",
        ));
    }

    let mut task = tauri::async_runtime::spawn_blocking(crate::devices::enumerate);
    match tokio::time::timeout(AUDIO_ENUMERATION_TIMEOUT, &mut task).await {
        Ok(Ok(devices)) => {
            running.store(false, Ordering::Release);
            Ok(devices)
        }
        Ok(Err(e)) => {
            running.store(false, Ordering::Release);
            Err(AppError::internal(format!("enumerate audio devices: {e}")))
        }
        Err(_) => {
            let cleanup = Arc::clone(&running);
            tauri::async_runtime::spawn(async move {
                let _ = task.await;
                cleanup.store(false, Ordering::Release);
            });
            Err(AppError::internal(
                "audio device enumeration timed out after 3s; CoreAudio may be busy",
            ))
        }
    }
}

fn set_driver_state_from_devices(h: &AppHandle, devices: &AudioDevices) -> DriverState {
    let driver_state = crate::driver_status::state_from_devices(devices);
    *h.driver_state.lock().unwrap() = driver_state;
    h.state.lock().unwrap().status.virtual_mic_installed = driver_state == DriverState::Healthy;
    sync_source_mic_name_from_devices(h, devices);
    driver_state
}

fn device_label_from_id(device_id: &str) -> String {
    device_id
        .strip_prefix("coreaudio:")
        .unwrap_or(device_id)
        .to_string()
}

fn sync_source_mic_name_from_devices(h: &AppHandle, devices: &AudioDevices) {
    let selected_id = h.config.lock().unwrap().audio.source_mic_id.clone();
    let selected_name = selected_id.as_ref().map(|id| {
        devices
            .inputs
            .iter()
            .find(|device| device.id == *id)
            .map(|device| device.name.clone())
            .unwrap_or_else(|| device_label_from_id(id))
    });
    h.state.lock().unwrap().status.source_mic_name = selected_name;
}

/// Persist the current config to disk. Must be called **after** dropping any
/// `MutexGuard` on `h.config` to avoid a deadlock.
fn update_config<F>(h: &AppHandle, f: F) -> Result<Config, AppError>
where
    F: FnOnce(&mut Config),
{
    let mut guard = h.config.lock().unwrap();
    let mut next = guard.clone();
    f(&mut next);
    next.validate()?;
    crate::appcfg::persist(&next)?;
    *guard = next.clone();
    Ok(next)
}

// ── Tray helpers (pure, no Tauri runtime) ────────────────────────────────────

/// Short label shown in the tray title / tooltip for each mode.
/// Order: [Silence, PassThrough, Translate].
pub fn tray_mode_label(mode: VirtualMicMode) -> &'static str {
    match mode {
        VirtualMicMode::Silence => "Silence",
        VirtualMicMode::PassThrough => "Pass-Through",
        VirtualMicMode::Translate => "Translate",
    }
}

/// Returns `[silence_checked, passthrough_checked, translate_checked]`.
pub fn tray_menu_checks(current: VirtualMicMode) -> [bool; 3] {
    [
        current == VirtualMicMode::Silence,
        current == VirtualMicMode::PassThrough,
        current == VirtualMicMode::Translate,
    ]
}

// ── DRY mode-application helper ───────────────────────────────────────────────

/// Apply a mode change: persist config, update AppState, drive engine, emit
/// `status-changed`, then refresh the tray title and checked-state.
///
/// Takes plain refs (NOT `tauri::State`) so it can be called from both the
/// `#[tauri::command]` (which resolves its `tauri::State` params first) and
/// the tray `on_menu_event` closure (which resolves state via `app.state()`).
///
pub fn apply_mode(
    app: &tauri::AppHandle,
    h: &AppHandle,
    engine: &std::sync::Arc<crate::engine::Engine>,
    mode: VirtualMicMode,
) -> Result<(), AppError> {
    let mode_string = serde_json::to_value(mode)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    update_config(h, |cfg| {
        cfg.audio.virtual_mic_mode = mode_string;
    })?;

    h.state.lock().unwrap().transition(mode);

    engine.set_mode(mode);

    let status = h.state.lock().unwrap().status.clone();
    {
        use tauri::Emitter;
        let _ = app.emit("status-changed", status);
    }

    // 6. Update tray title + checked-state (best-effort; errors ignored).
    // FIX-5: never hold two MutexGuards at once — read each field with a
    // single-lock statement so the guard is dropped at the semicolon.
    use tauri::Manager as _;
    if let Some(tray_state) = app.try_state::<TrayState>() {
        let label = tray_mode_label(mode);
        let show_badge = h.config.lock().unwrap().ui.show_latency_badge; // guard dropped at ;
        let latency = h.state.lock().unwrap().status.latency_ms;          // guard dropped at ;
        let title = crate::platform_integration::tray_title(label, show_badge, latency);
        let _ = tray_state.tray.set_title(Some(title.as_str()));
        let checks = tray_menu_checks(mode);
        let _ = tray_state.mode_silence.set_checked(checks[0]);
        let _ = tray_state.mode_passthrough.set_checked(checks[1]);
        let _ = tray_state.mode_translate.set_checked(checks[2]);
    }
    Ok(())
}

// ── Tray managed state ─────────────────────────────────────────────────────────

/// Holds the `TrayIcon` handle plus the 3 `CheckMenuItem` handles so that
/// `apply_mode` can update their checked-state and the tray title without
/// rebuilding the whole menu. `TrayIcon<Wry>` is `Send + Sync` (Tauri marks
/// it so); `CheckMenuItem<Wry>` likewise. Stored as Tauri managed state.
pub struct TrayState {
    pub tray: tauri::tray::TrayIcon,
    pub mode_silence: tauri::menu::CheckMenuItem<tauri::Wry>,
    pub mode_passthrough: tauri::menu::CheckMenuItem<tauri::Wry>,
    pub mode_translate: tauri::menu::CheckMenuItem<tauri::Wry>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioLevels {
    pub input_level: f32,
    pub output_level: f32,
}

#[tauri::command]
pub fn get_app_status(
    h: tauri::State<AppHandle>,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<AppStatus, AppError> {
    let installed = *h.driver_state.lock().unwrap() == DriverState::Healthy;
    let selected_id = h.config.lock().unwrap().audio.source_mic_id.clone();
    let (input_level, output_level) = engine.levels();
    let mut st = h.state.lock().unwrap();
    st.status.virtual_mic_installed = installed;
    st.status.input_level = input_level;
    st.status.output_level = output_level;
    if st.status.source_mic_name.is_none() {
        st.status.source_mic_name = selected_id.as_deref().map(device_label_from_id);
    }
    Ok(st.status.clone())
}

#[tauri::command]
pub fn set_virtual_mic_mode(
    mode: VirtualMicMode,
    app: tauri::AppHandle,
    h: tauri::State<'_, AppHandle>,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    apply_mode(&app, &h, &engine, mode)
}

#[tauri::command]
pub async fn get_audio_devices(h: tauri::State<'_, AppHandle>) -> Result<AudioDevices, AppError> {
    let devices = enumerate_audio_devices_bounded(Arc::clone(&h.audio_enumeration_running)).await?;
    set_driver_state_from_devices(&h, &devices);
    Ok(devices)
}

#[tauri::command]
pub fn get_audio_levels(
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<AudioLevels, AppError> {
    let (input_level, output_level) = engine.levels();
    Ok(AudioLevels {
        input_level,
        output_level,
    })
}

#[tauri::command]
pub fn set_source_mic(
    device_id: String,
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    update_config(&h, |cfg| {
        cfg.audio.source_mic_id = Some(device_id.clone());
    })?;
    h.state.lock().unwrap().status.source_mic_name = Some(device_label_from_id(&device_id));
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
    update_config(&h, |cfg| {
        cfg.audio.monitor_output_id = device_id;
    })?;
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
    let tgt = {
        update_config(&h, |cfg| {
            cfg.translation.target_language = language.clone();
        })?;
        language.clone()
    };
    {
        h.state.lock().unwrap().status.target_language = language;
    } // state MutexGuard dropped
    // Drive the engine — only restarts an active OpenAI session; no-op in Silence/PassThrough.
    engine.set_target_language(tgt);
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
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    update_config(&h, |cfg| {
        cfg.mix.duck_original = settings.duck_original;
        cfg.audio.limiter_enabled = settings.limiter_enabled;
    })?;
    engine.restart_translation_session_for_config();
    {
        use tauri::Emitter;
        let _ = app.emit("status-changed", h.state.lock().unwrap().status.clone());
    }
    Ok(())
}

#[tauri::command]
pub async fn install_virtual_mic(h: tauri::State<'_, AppHandle>) -> Result<(), AppError> {
    crate::driver_status::install().map_err(AppError::internal)?;
    let devices = enumerate_audio_devices_bounded(Arc::clone(&h.audio_enumeration_running)).await?;
    // Re-check driver state after the privileged install.
    match set_driver_state_from_devices(&h, &devices) {
        DriverState::Healthy => Ok(()),
        _ => Err(AppError::driver_missing()),
    }
}

#[tauri::command]
pub async fn update_virtual_mic(h: tauri::State<'_, AppHandle>) -> Result<(), AppError> {
    // "Update" == reinstall the bundled driver.
    crate::driver_status::install().map_err(AppError::internal)?;
    let devices = enumerate_audio_devices_bounded(Arc::clone(&h.audio_enumeration_running)).await?;
    match set_driver_state_from_devices(&h, &devices) {
        DriverState::Healthy => Ok(()),
        _ => Err(AppError::driver_missing()),
    }
}

#[tauri::command]
pub fn uninstall_virtual_mic(h: tauri::State<AppHandle>) -> Result<(), AppError> {
    crate::driver_status::uninstall().map_err(AppError::internal)?;
    *h.driver_state.lock().unwrap() = DriverState::Missing;
    h.state.lock().unwrap().status.virtual_mic_installed = false;
    Ok(())
}

#[tauri::command]
pub fn get_driver_state(h: tauri::State<AppHandle>) -> Result<DriverState, AppError> {
    Ok(*h.driver_state.lock().unwrap())
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
pub async fn open_system_mic_permission_settings() -> Result<MicPermission, AppError> {
    let permission = tauri::async_runtime::spawn_blocking(crate::permission::request_access)
        .await
        .map_err(|e| AppError::internal(format!("request microphone permission: {e}")))?;
    if permission != MicPermission::Granted {
        crate::permission::open_privacy_pane();
    }
    Ok(permission)
}

#[tauri::command]
pub fn get_mic_permission() -> Result<MicPermission, AppError> {
    Ok(crate::permission::status())
}

#[tauri::command]
pub async fn request_mic_permission() -> Result<MicPermission, AppError> {
    tauri::async_runtime::spawn_blocking(crate::permission::request_access)
        .await
        .map_err(|e| AppError::internal(format!("request microphone permission: {e}")))
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

#[tauri::command]
pub fn start_mic_level_probe(
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    engine.start_level_probe()
}

#[tauri::command]
pub fn stop_mic_level_probe(
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    engine.stop_level_probe();
    Ok(())
}

/// Clear on-disk transcript history (Task 8) and reset the live in-session
/// transcript buffers held by the frontend store.
///
/// Ends any active session log first (so an in-flight session's file is also
/// cleared and not re-created — FIX-T8), deletes every `.jsonl` file under
/// the transcripts directory, then emits `"transcript-cleared"` so the
/// frontend store zeroes `srcText`/`tgtText`. Returns the number of files deleted.
#[tauri::command]
pub fn clear_transcript_history(
    app: tauri::AppHandle,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<usize, AppError> {
    use tauri::Emitter;
    engine.end_session_log();
    let n = crate::transcript_log::clear_all();
    let _ = app.emit("transcript-cleared", ());
    Ok(n)
}

#[tauri::command]
pub fn stop_all_audio(
    app: tauri::AppHandle,
    h: tauri::State<'_, AppHandle>,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    engine.stop_level_probe();
    apply_mode(&app, &h, &engine, VirtualMicMode::Silence)
}

// ── Account / API key ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub has_key: bool,
    pub verified: bool,
    pub masked_key: Option<String>,
    pub last_verified: Option<String>,
    pub month_minutes: f64,
    pub month_usd: f64,
    pub total_minutes: f64,
    pub total_usd: f64,
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

/// NOTE: performs a synchronous disk read (`usage_store::load()`). Callers are
/// startup + user-triggered key ops only — do NOT call this on a poll loop.
fn account_status(account: &AccountConfig) -> AccountStatus {
    let key = account.openai_api_key.as_deref().filter(|s| !s.is_empty());
    let masked = key.map(mask_key);
    let u = crate::usage_store::load();
    AccountStatus {
        has_key: key.is_some(),
        verified: key.is_some() && account.openai_api_key_verified,
        masked_key: masked,
        last_verified: account.openai_api_key_last_verified.clone(),
        month_minutes: u.month_minutes(),
        month_usd: u.month_usd(),
        total_minutes: u.total_minutes(),
        total_usd: u.total_usd(),
    }
}

#[tauri::command]
pub fn get_account_status(h: tauri::State<AppHandle>) -> Result<AccountStatus, AppError> {
    Ok(account_status(&h.config.lock().unwrap().account))
}

#[tauri::command]
pub fn set_api_key(h: tauri::State<AppHandle>, key: String) -> Result<AccountStatus, AppError> {
    let key = key.trim();
    let cfg = update_config(&h, |cfg| {
        cfg.account.openai_api_key = (!key.is_empty()).then(|| key.to_string());
        cfg.account.openai_api_key_verified = false;
        cfg.account.openai_api_key_last_verified = None;
    })?;
    Ok(account_status(&cfg.account))
}

#[tauri::command]
pub async fn verify_api_key(h: tauri::State<'_, AppHandle>) -> Result<AccountStatus, AppError> {
    use intervox_core::realtime::openai_auth::{
        classify_validation, KeyValidation, VALIDATION_URL,
    };

    let key = h
        .config
        .lock()
        .unwrap()
        .account
        .openai_api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AppError::openai_auth_error("No API key set"))?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| AppError::internal(e.to_string()))?;

    let http_status = match client.get(VALIDATION_URL).bearer_auth(&key).send().await {
        Ok(resp) => Some(resp.status().as_u16()),
        Err(_) => None,
    };

    match classify_validation(http_status) {
        KeyValidation::Verified => {
            let still_current = h
                .config
                .lock()
                .unwrap()
                .account
                .openai_api_key
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                == Some(key.as_str());
            if !still_current {
                let cfg = h.config.lock().unwrap().clone();
                return Ok(account_status(&cfg.account));
            }
            let last_verified = rfc3339_now();
            let cfg = update_config(&h, |cfg| {
                cfg.account.openai_api_key_verified = true;
                cfg.account.openai_api_key_last_verified = Some(last_verified);
            })?;
            Ok(account_status(&cfg.account))
        }
        KeyValidation::InvalidKey => {
            let still_current = h
                .config
                .lock()
                .unwrap()
                .account
                .openai_api_key
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                == Some(key.as_str());
            if !still_current {
                let cfg = h.config.lock().unwrap().clone();
                return Ok(account_status(&cfg.account));
            }
            update_config(&h, |cfg| {
                cfg.account.openai_api_key_verified = false;
                cfg.account.openai_api_key_last_verified = None;
            })?;
            Err(AppError::openai_auth_error("Invalid API key"))
        }
        KeyValidation::Offline | KeyValidation::Unknown => {
            let cfg = h.config.lock().unwrap().clone();
            Ok(account_status(&cfg.account))
        }
    }
}

/// Returns the current UTC time formatted as an RFC 3339 string,
/// e.g. `2026-05-16T12:34:56Z`. Uses only `std::time` — no extra crate dep.
pub(crate) fn rfc3339_now() -> String {
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
pub fn clear_api_key(h: tauri::State<AppHandle>) -> Result<(), AppError> {
    update_config(&h, |cfg| {
        cfg.account = AccountConfig::default();
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{mask_key, test_tone, tray_menu_checks, tray_mode_label};
    use intervox_core::state::VirtualMicMode;

    // ── tray_mode_label ───────────────────────────────────────────────────────

    #[test]
    fn tray_mode_label_all_three_distinct() {
        let labels = [
            tray_mode_label(VirtualMicMode::Silence),
            tray_mode_label(VirtualMicMode::PassThrough),
            tray_mode_label(VirtualMicMode::Translate),
        ];
        // All three labels must be non-empty and distinct.
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
    }

    // ── tray_menu_checks ──────────────────────────────────────────────────────

    #[test]
    fn tray_menu_checks_exactly_one_true_per_mode() {
        let modes = [
            VirtualMicMode::Silence,
            VirtualMicMode::PassThrough,
            VirtualMicMode::Translate,
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
        // Order: [Silence=0, PassThrough=1, Translate=2]
        assert_eq!(
            tray_menu_checks(VirtualMicMode::Silence),
            [true, false, false]
        );
        assert_eq!(
            tray_menu_checks(VirtualMicMode::PassThrough),
            [false, true, false]
        );
        assert_eq!(
            tray_menu_checks(VirtualMicMode::Translate),
            [false, false, true]
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
        assert_eq!(
            masked,
            format!("{expected_prefix}{bullets}{expected_suffix}")
        );
        // Bullet count should be exactly 20.
        assert_eq!(masked.chars().filter(|&c| c == '\u{2022}').count(), 20);
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
        assert_eq!(
            tone.len(),
            48_000,
            "1 s at 48 kHz must produce exactly 48 000 samples"
        );
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
    let mut cfg = h.config.lock().unwrap().clone();
    cfg.account.openai_api_key = None;
    Ok(cfg)
}

#[tauri::command]
pub fn set_quality_mode(quality: String, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    update_config(&h, |cfg| {
        cfg.translation.quality_mode = quality;
    })?;
    Ok(())
}

#[tauri::command]
pub fn set_mix_percent(
    percent: u32,
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    let clamped = percent.min(30);
    update_config(&h, |cfg| {
        cfg.mix.original_voice_percent = clamped;
    })?;
    engine.restart_translation_session_for_config();
    {
        use tauri::Emitter;
        let _ = app.emit("status-changed", h.state.lock().unwrap().status.clone());
    }
    Ok(())
}

#[tauri::command]
pub fn set_captions_config(c: CaptionsConfig, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    update_config(&h, |cfg| {
        cfg.captions = c;
    })?;
    Ok(())
}

#[tauri::command]
pub fn set_privacy_config(p: PrivacyConfig, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    update_config(&h, |cfg| {
        cfg.privacy = p;
    })?;
    Ok(())
}

#[tauri::command]
pub fn set_shortcuts(
    s: ShortcutsConfig,
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
) -> Result<(), AppError> {
    update_config(&h, |cfg| {
        cfg.shortcuts = s;
    })?;
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
    update_config(&h, |cfg| {
        cfg.onboarding_completed = true;
    })?;
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
        win.close()
            .map_err(|e| AppError::internal(format!("close captions: {e}")))?;
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

// ── Connection log / UI config / external URL ────────────────────────────────

/// Return a snapshot of the in-memory connection lifecycle ring.
#[tauri::command]
pub fn get_connection_log(
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<Vec<crate::connection_log::ConnLogEntry>, AppError> {
    Ok(engine.conn_log().snapshot())
}

/// Persist a new `UiConfig` to disk and apply OS integrations immediately.
#[tauri::command]
pub fn set_ui_config(
    ui: intervox_core::config::UiConfig,
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
    engine: tauri::State<'_, std::sync::Arc<crate::engine::Engine>>,
) -> Result<(), AppError> {
    update_config(&h, |cfg| {
        cfg.ui = ui.clone();
    })?;
    crate::platform_integration::apply_ui_config(&app, &ui);
    // FIX-3d: propagate badge toggle to the pull_task atomic so the next ~1 Hz
    // tick uses the new value without any disk read.
    engine.set_show_latency_badge(ui.show_latency_badge);
    Ok(())
}

/// Open an https:// URL in the default browser via `open`.
/// Non-https URLs are refused to prevent unintended protocol handlers.
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), AppError> {
    if !url.starts_with("https://") {
        return Err(AppError::internal("refusing to open non-https URL"));
    }
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| AppError::internal(e.to_string()))?;
    Ok(())
}

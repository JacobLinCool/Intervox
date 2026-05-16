//! Tauri command surface (spec §4.1). The frontend never touches audio or
//! OpenAI directly — every native operation goes through these. Audio capture,
//! the websocket transport and the HAL driver are not wired yet, so device
//! enumeration is mocked; state/config/mode logic is the real core.

use intervox_core::audio::mixer::MixSettings;
use intervox_core::config::{CaptionsConfig, PrivacyConfig, ShortcutsConfig};
use intervox_core::state::{AppState, AppStatus, VirtualMicMode};
use intervox_core::{AppError, Config};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Default)]
pub struct AppHandle {
    pub state: Mutex<AppState>,
    pub config: Mutex<Config>,
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
    Ok(h.state.lock().unwrap().status.clone())
}

#[tauri::command]
pub fn set_virtual_mic_mode(
    mode: VirtualMicMode,
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
) -> Result<(), AppError> {
    h.state.lock().unwrap().transition(mode);
    h.config.lock().unwrap().audio.virtual_mic_mode =
        serde_json::to_value(mode).unwrap().as_str().unwrap().to_string();
    {
        use tauri::Emitter;
        let _ = app.emit("status-changed", h.state.lock().unwrap().status.clone());
    }
    Ok(())
}

#[tauri::command]
pub fn get_audio_devices() -> Result<AudioDevices, AppError> {
    // Mocked until real CoreAudio/CPAL enumeration lands (deferred milestone).
    Ok(AudioDevices {
        inputs: vec![
            DeviceInfo {
                id: "coreaudio:builtin-mic".into(),
                name: "Built-in Microphone (mock)".into(),
            },
            DeviceInfo {
                id: "coreaudio:usb-mic".into(),
                name: "USB Microphone (mock)".into(),
            },
        ],
        outputs: vec![DeviceInfo {
            id: "coreaudio:builtin-out".into(),
            name: "Built-in Output (mock)".into(),
        }],
    })
}

#[tauri::command]
pub fn set_source_mic(
    device_id: String,
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
) -> Result<(), AppError> {
    h.config.lock().unwrap().audio.source_mic_id = Some(device_id.clone());
    h.state.lock().unwrap().status.source_mic_name = Some(device_id);
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
    Ok(())
}

#[tauri::command]
pub fn set_target_language(
    language: String,
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
) -> Result<(), AppError> {
    h.config.lock().unwrap().translation.target_language = language.clone();
    h.state.lock().unwrap().status.target_language = language;
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
    {
        use tauri::Emitter;
        let _ = app.emit("status-changed", h.state.lock().unwrap().status.clone());
    }
    Ok(())
}

#[tauri::command]
pub fn install_virtual_mic() -> Result<(), AppError> {
    // Deferred: privileged helper + HAL .driver install needs codesign/
    // notarization on the user machine. Surface the spec error contract.
    Err(AppError::driver_missing())
}

#[tauri::command]
pub fn uninstall_virtual_mic() -> Result<(), AppError> {
    Ok(())
}

#[tauri::command]
pub fn open_system_mic_permission_settings() -> Result<(), AppError> {
    // Deferred: will open x-apple.systempreferences:com.apple.preference
    // .security?Privacy_Microphone once capture is wired.
    Ok(())
}

#[tauri::command]
pub fn start_test_phrase() -> Result<(), AppError> {
    Ok(())
}

#[tauri::command]
pub fn stop_all_audio(
    app: tauri::AppHandle,
    h: tauri::State<AppHandle>,
) -> Result<(), AppError> {
    h.state.lock().unwrap().transition(VirtualMicMode::Silence);
    {
        use tauri::Emitter;
        let _ = app.emit("status-changed", h.state.lock().unwrap().status.clone());
    }
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

fn api_key_path() -> PathBuf {
    PathBuf::from("apikey.secret")
}

fn read_api_key() -> Option<String> {
    fs::read_to_string(api_key_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn account_status(verified: bool) -> AccountStatus {
    let key = read_api_key();
    let masked = key.as_ref().map(|k| {
        if k.len() > 11 {
            format!("{}{}{}", &k[..7], "\u{2022}".repeat(20), &k[k.len() - 4..])
        } else {
            "\u{2022}".repeat(8)
        }
    });
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
    let trimmed = key.trim().to_string();
    fs::write(api_key_path(), &trimmed)
        .map_err(|e| AppError::internal(format!("cannot write api key: {e}")))?;
    Ok(account_status(false))
}

#[tauri::command]
pub fn verify_api_key() -> Result<AccountStatus, AppError> {
    let verified = read_api_key()
        .map(|k| k.starts_with("sk-") && k.len() >= 23)
        .unwrap_or(false);
    Ok(account_status(verified))
}

#[tauri::command]
pub fn clear_api_key() -> Result<(), AppError> {
    match fs::remove_file(api_key_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AppError::internal(format!("cannot remove api key: {e}"))),
    }
}

// ── Config commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_config(h: tauri::State<AppHandle>) -> Result<Config, AppError> {
    Ok(h.config.lock().unwrap().clone())
}

#[tauri::command]
pub fn set_source_language(language: String, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    h.config.lock().unwrap().translation.source_language = language;
    Ok(())
}

#[tauri::command]
pub fn set_quality_mode(quality: String, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    h.config.lock().unwrap().translation.quality_mode = quality;
    Ok(())
}

#[tauri::command]
pub fn set_mix_percent(percent: u32, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    let clamped = percent.min(30);
    h.config.lock().unwrap().mix.original_voice_percent = clamped;
    Ok(())
}

#[tauri::command]
pub fn set_captions_config(c: CaptionsConfig, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    h.config.lock().unwrap().captions = c;
    Ok(())
}

#[tauri::command]
pub fn set_privacy_config(p: PrivacyConfig, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    h.config.lock().unwrap().privacy = p;
    Ok(())
}

#[tauri::command]
pub fn set_shortcuts(s: ShortcutsConfig, h: tauri::State<AppHandle>) -> Result<(), AppError> {
    h.config.lock().unwrap().shortcuts = s;
    Ok(())
}

#[tauri::command]
pub fn complete_onboarding(h: tauri::State<AppHandle>) -> Result<(), AppError> {
    h.config.lock().unwrap().onboarding_completed = true;
    Ok(())
}

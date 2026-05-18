//! Config schema (spec §10). Persisted as JSON. Rust validates and is the
//! source of truth; the engineering layer always works in dB internally even
//! though the UI surfaces percentages (spec §2.4).

use crate::errors::AppError;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const CONFIG_VERSION: u32 = 1;

/// Convert a UI volume percentage (0–100) to dB. 0% maps to a practical
/// silence floor rather than -inf so it round-trips and stays finite.
pub fn percent_to_db(percent: f32) -> f32 {
    if percent <= 0.0 {
        -120.0
    } else {
        20.0 * (percent / 100.0).log10()
    }
}

pub fn db_to_percent(db: f32) -> f32 {
    100.0 * 10f32.powf(db / 20.0)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub source_id: Option<String>,
    pub output_preview_enabled: bool,
    pub virtual_mic_mode: String,
    pub input_gain_db: f32,
    pub limiter_enabled: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            source_id: None,
            output_preview_enabled: false,
            virtual_mic_mode: "silence".into(),
            input_gain_db: 0.0,
            limiter_enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslationConfig {
    // NOTE: there is intentionally no `source_language`. The OpenAI realtime
    // translation endpoint auto-detects the source language and does not accept
    // it as a parameter, so exposing it would only mislead users.
    pub target_language: String,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            target_language: "en".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MixConfig {
    pub original_voice_percent: u32,
    pub translated_voice_percent: u32,
    pub duck_original: bool,
}

impl Default for MixConfig {
    fn default() -> Self {
        Self {
            original_voice_percent: 0,
            translated_voice_percent: 100,
            duck_original: true,
        }
    }
}

/// Logical-pixel bounds for the captions window width. Kept here (not just in
/// the Tauri layer) so persisted geometry can be clamped before it is ever
/// applied to a window, and so the clamp is unit-testable without a GUI.
pub const CAPTIONS_MIN_WIDTH: f64 = 420.0;
pub const CAPTIONS_MAX_WIDTH: f64 = 920.0;

/// Clamp a persisted/raw captions window width into the supported range.
/// Non-finite input (NaN/inf from a corrupt config) falls back to the min.
pub fn clamp_captions_window_width(width: f64) -> f64 {
    if !width.is_finite() {
        return CAPTIONS_MIN_WIDTH;
    }
    width.clamp(CAPTIONS_MIN_WIDTH, CAPTIONS_MAX_WIDTH)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptionsConfig {
    pub enabled: bool,
    pub show_source: bool,
    pub show_target: bool,
    pub font_size: String,
    /// Keep the captions window above other windows and visible over another
    /// app's native macOS fullscreen Space (see `captions_overlay`).
    pub always_on_top: bool,
    /// Persisted window placement (logical pixels). `None` until the user has
    /// moved/resized the window at least once, in which case the OS default
    /// placement is used. Height is intentionally not persisted because it is
    /// driven by the compact/expanded toggle.
    pub window_x: Option<f64>,
    pub window_y: Option<f64>,
    pub window_width: Option<f64>,
}

impl Default for CaptionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_source: false,
            show_target: true,
            font_size: "medium".into(),
            always_on_top: true,
            window_x: None,
            window_y: None,
            window_width: None,
        }
    }
}

impl CaptionsConfig {
    /// Restored placement to apply when (re)creating the window: `(x, y)` is
    /// only returned when both coordinates are present; width is clamped to the
    /// supported range. Returns `None` for each part that was never persisted.
    pub fn restored_placement(&self) -> (Option<(f64, f64)>, Option<f64>) {
        let position = match (self.window_x, self.window_y) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Some((x, y)),
            _ => None,
        };
        let width = self.window_width.map(clamp_captions_window_width);
        (position, width)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyConfig {
    pub save_transcript_history: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            save_transcript_history: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountConfig {
    pub openai_api_key: Option<String>,
    pub openai_api_key_verified: bool,
    pub openai_api_key_last_verified: Option<String>,
}

/// Default inactivity-reminder period in minutes (issue #2). `0` disables it.
pub const DEFAULT_INACTIVITY_REMINDER_MINUTES: u32 = 10;
/// Upper bound for the inactivity-reminder period (24 h).
pub const MAX_INACTIVITY_REMINDER_MINUTES: u32 = 24 * 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub show_latency_badge: bool,
    pub launch_at_login: bool,
    pub hide_dock_icon: bool,
    /// Minutes of no interpreted text before a silent inactivity reminder is
    /// shown while Interpret is on. `0` disables the inactivity reminder.
    /// The recurring hourly duration reminder is not configurable.
    pub inactivity_reminder_minutes: u32,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_latency_badge: false,
            launch_at_login: false,
            hide_dock_icon: false,
            inactivity_reminder_minutes: DEFAULT_INACTIVITY_REMINDER_MINUTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutsConfig {
    pub toggle_translate: String,
    pub silence: String,
    pub captions: String,
}

impl Default for ShortcutsConfig {
    fn default() -> Self {
        Self {
            toggle_translate: "Cmd+Shift+T".into(),
            silence: "Cmd+Shift+M".into(),
            captions: "Cmd+Shift+C".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub translation: TranslationConfig,
    #[serde(default)]
    pub mix: MixConfig,
    #[serde(default)]
    pub captions: CaptionsConfig,
    #[serde(default)]
    pub privacy: PrivacyConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub account: AccountConfig,
    #[serde(default)]
    pub shortcuts: ShortcutsConfig,
    #[serde(default)]
    pub onboarding_completed: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            audio: AudioConfig::default(),
            translation: TranslationConfig::default(),
            mix: MixConfig::default(),
            captions: CaptionsConfig::default(),
            privacy: PrivacyConfig::default(),
            ui: UiConfig::default(),
            account: AccountConfig::default(),
            shortcuts: ShortcutsConfig::default(),
            onboarding_completed: false,
        }
    }
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| AppError::invalid_config(format!("cannot read config: {e}")))?;
        let cfg: Config = serde_json::from_str(&text)
            .map_err(|e| AppError::invalid_config(format!("cannot parse config: {e}")))?;
        let mut cfg = cfg;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), AppError> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::internal(format!("serialize config: {e}")))?;
        let path = path.as_ref();
        std::fs::write(path, text)
            .map_err(|e| AppError::invalid_config(format!("cannot write config: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// Clamp/repair out-of-range values per spec UI limits (original voice
    /// 0–30%). Returns an error only for structurally invalid input.
    pub fn validate(&mut self) -> Result<(), AppError> {
        if self.version != CONFIG_VERSION {
            return Err(AppError::invalid_config(format!(
                "unsupported config version {}",
                self.version
            )));
        }
        match self.audio.virtual_mic_mode.as_str() {
            "silence" | "pass_through" | "translate" => {}
            mode => {
                return Err(AppError::invalid_config(format!(
                    "unsupported virtual_mic_mode {mode}"
                )));
            }
        }
        self.mix.original_voice_percent = self.mix.original_voice_percent.min(30);
        self.mix.translated_voice_percent = self.mix.translated_voice_percent.min(100);
        self.ui.inactivity_reminder_minutes = self
            .ui
            .inactivity_reminder_minutes
            .min(MAX_INACTIVITY_REMINDER_MINUTES);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_spec_section_10() {
        let c = Config::default();
        assert_eq!(c.version, 1);
        assert_eq!(c.audio.source_id, None);
        assert_eq!(c.audio.virtual_mic_mode, "silence");
        assert!(!c.audio.output_preview_enabled);
        assert!(c.audio.limiter_enabled);
        assert_eq!(c.translation.target_language, "en");
        assert_eq!(c.mix.original_voice_percent, 0);
        assert_eq!(c.mix.translated_voice_percent, 100);
        assert!(c.mix.duck_original);
        assert!(c.captions.enabled);
        assert!(c.privacy.save_transcript_history);
        assert!(c.account.openai_api_key.is_none());
        assert!(!c.account.openai_api_key_verified);
        assert_eq!(c.shortcuts.toggle_translate, "Cmd+Shift+T");
    }

    #[test]
    fn percent_db_round_trip() {
        let db = percent_to_db(15.0);
        assert!((db - (-16.478)).abs() < 0.01, "got {db}");
        let p = db_to_percent(db);
        assert!((p - 15.0).abs() < 0.015, "got {p}");
    }

    #[test]
    fn twelve_point_five_percent_is_about_minus_18db() {
        assert!((percent_to_db(12.5) - (-18.06)).abs() < 0.05);
    }

    #[test]
    fn validate_clamps_original_voice() {
        let mut c = Config::default();
        c.mix.original_voice_percent = 99;
        c.validate().unwrap();
        assert_eq!(c.mix.original_voice_percent, 30);
    }

    #[test]
    fn validate_rejects_removed_original_mix_mode() {
        let mut c = Config::default();
        c.audio.virtual_mic_mode = "translate_with_original".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_unknown_version() {
        let mut c = Config {
            version: 99,
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn save_load_round_trips() {
        let mut path = std::env::temp_dir();
        path.push(format!("intervox-cfg-{}.json", std::process::id()));
        let mut c = Config::default();
        c.translation.target_language = "ja".into();
        c.account.openai_api_key = Some("sk-test-config-roundtrip".into());
        c.account.openai_api_key_verified = true;
        c.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded, c);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn partial_json_uses_defaults_for_missing_sections() {
        let cfg: Config = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert_eq!(cfg.mix.original_voice_percent, 0);
        assert_eq!(cfg.translation.target_language, "en");
    }

    #[test]
    fn unknown_future_fields_do_not_crash() {
        let cfg: Config =
            serde_json::from_str(r#"{"version":1,"audio":{"future_field":42}}"#).unwrap();
        assert_eq!(cfg.audio.virtual_mic_mode, "silence");
    }

    #[test]
    fn ui_config_defaults_off() {
        let c = Config::default();
        assert!(!c.ui.show_latency_badge);
        assert!(!c.ui.launch_at_login);
        assert!(!c.ui.hide_dock_icon);
        assert_eq!(
            c.ui.inactivity_reminder_minutes,
            DEFAULT_INACTIVITY_REMINDER_MINUTES
        );
    }

    #[test]
    fn inactivity_reminder_minutes_defaults_when_missing_from_json() {
        // Older configs written before issue #2 have no ui.inactivity field;
        // they must come back with the sensible default, not 0 (disabled).
        let cfg: Config =
            serde_json::from_str(r#"{"version":1,"ui":{"show_latency_badge":true}}"#).unwrap();
        assert!(cfg.ui.show_latency_badge);
        assert_eq!(
            cfg.ui.inactivity_reminder_minutes,
            DEFAULT_INACTIVITY_REMINDER_MINUTES
        );
    }

    #[test]
    fn validate_clamps_inactivity_reminder_minutes() {
        let mut c = Config::default();
        c.ui.inactivity_reminder_minutes = 999_999;
        c.validate().unwrap();
        assert_eq!(
            c.ui.inactivity_reminder_minutes,
            MAX_INACTIVITY_REMINDER_MINUTES
        );
        // 0 (disabled) is valid and preserved.
        c.ui.inactivity_reminder_minutes = 0;
        c.validate().unwrap();
        assert_eq!(c.ui.inactivity_reminder_minutes, 0);
    }

    #[test]
    fn captions_geometry_defaults_unset() {
        let c = CaptionsConfig::default();
        assert_eq!(c.window_x, None);
        assert_eq!(c.window_y, None);
        assert_eq!(c.window_width, None);
        let (pos, width) = c.restored_placement();
        assert_eq!(pos, None);
        assert_eq!(width, None);
    }

    #[test]
    fn clamp_captions_window_width_bounds() {
        assert_eq!(clamp_captions_window_width(100.0), CAPTIONS_MIN_WIDTH);
        assert_eq!(clamp_captions_window_width(5000.0), CAPTIONS_MAX_WIDTH);
        assert_eq!(clamp_captions_window_width(640.0), 640.0);
        // Corrupt config values must not propagate NaN/inf into the window API.
        assert_eq!(clamp_captions_window_width(f64::NAN), CAPTIONS_MIN_WIDTH);
        assert_eq!(
            clamp_captions_window_width(f64::INFINITY),
            CAPTIONS_MIN_WIDTH
        );
    }

    #[test]
    fn restored_placement_clamps_width_and_requires_both_coords() {
        let c = CaptionsConfig {
            window_x: Some(120.0),
            window_y: None,
            window_width: Some(99_999.0),
            ..Default::default()
        };
        let (pos, width) = c.restored_placement();
        // y missing → position not restored, but a clamped width still is.
        assert_eq!(pos, None);
        assert_eq!(width, Some(CAPTIONS_MAX_WIDTH));

        let c2 = CaptionsConfig {
            window_x: Some(-40.0),
            window_y: Some(64.0),
            window_width: Some(700.0),
            ..Default::default()
        };
        let (pos2, width2) = c2.restored_placement();
        assert_eq!(pos2, Some((-40.0, 64.0)));
        assert_eq!(width2, Some(700.0));
    }

    #[test]
    fn captions_geometry_round_trips_through_json() {
        let mut path = std::env::temp_dir();
        path.push(format!("intervox-cap-geom-{}.json", std::process::id()));
        let mut c = Config::default();
        c.captions.window_x = Some(12.5);
        c.captions.window_y = Some(48.0);
        c.captions.window_width = Some(733.0);
        c.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.captions.window_x, Some(12.5));
        assert_eq!(loaded.captions.window_y, Some(48.0));
        assert_eq!(loaded.captions.window_width, Some(733.0));
        let _ = std::fs::remove_file(&path);
    }
}

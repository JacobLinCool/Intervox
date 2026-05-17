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
    pub source_mic_id: Option<String>,
    pub monitor_output_id: Option<String>,
    pub virtual_mic_mode: String,
    pub input_gain_db: f32,
    pub limiter_enabled: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            source_mic_id: None,
            monitor_output_id: None,
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
    pub quality_mode: String,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            target_language: "en".into(),
            quality_mode: "balanced".into(),
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptionsConfig {
    pub enabled: bool,
    pub show_source: bool,
    pub show_target: bool,
    pub font_size: String,
    pub always_on_top: bool,
}

impl Default for CaptionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_source: true,
            show_target: true,
            font_size: "medium".into(),
            always_on_top: true,
        }
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

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub show_latency_badge: bool,
    pub launch_at_login: bool,
    pub hide_dock_icon: bool,
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
        assert_eq!(c.audio.virtual_mic_mode, "silence");
        assert!(c.audio.limiter_enabled);
        assert_eq!(c.translation.target_language, "en");
        assert_eq!(c.translation.quality_mode, "balanced");
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
    }
}

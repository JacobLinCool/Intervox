//! App-data config persistence. Loads from / saves to the platform config dir.

use std::path::PathBuf;

use intervox_core::Config;

/// Returns the canonical config file path:
/// `~/Library/Application Support/app.intervox.desktop/config.json` on macOS.
pub fn config_path() -> PathBuf {
    let base =
        dirs::config_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"));
    base.join("app.intervox.desktop").join("config.json")
}

/// Load the persisted config, running `validate()` to clamp out-of-range
/// values. Falls back to `Config::default()` on any error (missing file,
/// parse failure, version mismatch, …).
pub fn load_or_default() -> Config {
    let p = config_path();
    match Config::load(&p) {
        Ok(mut c) => {
            let _ = c.validate();
            c
        }
        Err(_) => Config::default(),
    }
}

/// Persist `cfg` to the platform config dir, creating the directory if needed.
pub fn persist(cfg: &Config) -> Result<(), intervox_core::AppError> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| {
            intervox_core::AppError::invalid_config(format!("cannot create config dir: {e}"))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
                intervox_core::AppError::invalid_config(format!("cannot protect config dir: {e}"))
            })?;
        }
    }
    cfg.save(&p)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_target_language_and_mix_percent() {
        let mut path = std::env::temp_dir();
        path.push(format!("intervox-appcfg-test-{}.json", std::process::id()));

        let mut cfg = Config::default();
        cfg.translation.target_language = "ja".into();
        cfg.mix.original_voice_percent = 20;

        cfg.save(&path).unwrap();

        let loaded = Config::load(&path).unwrap();

        assert_eq!(
            loaded.translation.target_language, "ja",
            "target_language should round-trip"
        );
        assert_eq!(
            loaded.mix.original_voice_percent, 20,
            "original_voice_percent should round-trip"
        );

        let _ = std::fs::remove_file(&path);
    }
}

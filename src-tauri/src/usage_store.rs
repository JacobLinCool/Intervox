//! Persistence for UsageStore at
//! ~/Library/Application Support/app.intervox.desktop/usage.json (0600).

use intervox_core::usage::UsageStore;
use std::path::PathBuf;

fn usage_path() -> PathBuf {
    let base =
        dirs::config_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"));
    base.join("app.intervox.desktop").join("usage.json")
}

/// Convert Unix epoch seconds to a UTC "YYYY-MM" string (proleptic Gregorian).
fn month_from_secs(secs: u64) -> String {
    let days = secs / 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}")
}

/// Current UTC "YYYY-MM" using only std::time (no chrono dep — mirrors the
/// existing rfc3339_now approach in commands.rs).
pub fn current_month_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    month_from_secs(secs)
}

pub fn load() -> UsageStore {
    match std::fs::read_to_string(usage_path()) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_else(|e| {
            eprintln!("[usage_store] usage.json parse error (totals reset): {e}");
            UsageStore::default()
        }),
        Err(_) => UsageStore::default(),
    }
}

pub fn save(u: &UsageStore) {
    let p = usage_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(t) = serde_json::to_string_pretty(u) {
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, &t).is_ok() && std::fs::rename(&tmp, &p).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_format_is_yyyy_dash_mm() {
        let m = current_month_utc();
        assert_eq!(m.len(), 7);
        assert_eq!(&m[4..5], "-");
    }

    #[test]
    fn month_from_secs_known_values() {
        // 2026-05-01T00:00:00Z = 1_777_593_600 ; 2026-05-31T23:59:59Z = 1_780_271_999
        assert_eq!(super::month_from_secs(1_777_593_600), "2026-05");
        assert_eq!(super::month_from_secs(1_780_271_999), "2026-05");
        // 1970-01-01T00:00:00Z
        assert_eq!(super::month_from_secs(0), "1970-01");
        // 2000-03-01T00:00:00Z = 951_868_800 (leap-year boundary)
        assert_eq!(super::month_from_secs(951_868_800), "2000-03");
    }
}

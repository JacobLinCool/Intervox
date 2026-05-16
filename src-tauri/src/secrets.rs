use keyring::Entry;

const SERVICE: &str = "app.intervox.desktop";
const ACCOUNT: &str = "openai-api-key";

fn entry() -> keyring::Result<Entry> {
    Entry::new(SERVICE, ACCOUNT)
}

pub fn get_key() -> Option<String> {
    entry()
        .ok()?
        .get_password()
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn set_key(k: &str) -> Result<(), String> {
    entry()
        .map_err(|e| e.to_string())?
        .set_password(k.trim())
        .map_err(|e| e.to_string())
}

pub fn clear_key() -> Result<(), String> {
    match entry().and_then(|e| e.delete_credential()) {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// One-time migration: if a legacy `apikey.secret` exists and no Keychain key
/// is set, move it into the Keychain and delete the plaintext file.
pub fn migrate_legacy() {
    if get_key().is_some() {
        return;
    }
    if let Ok(s) = std::fs::read_to_string("apikey.secret") {
        let s = s.trim();
        if !s.is_empty() && set_key(s).is_ok() {
            let _ = std::fs::remove_file("apikey.secret");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real Keychain — may prompt on some systems.
    /// Run with: cargo test -- --ignored
    #[test]
    #[ignore]
    fn keychain_round_trip() {
        let test_key = "sk-test-keychain-round-trip-1234567890";
        // Clean up any leftover state first.
        let _ = clear_key();

        set_key(test_key).expect("set_key should succeed");
        let got = get_key().expect("get_key should return the key we just set");
        assert_eq!(got, test_key);

        clear_key().expect("clear_key should succeed");
        assert!(get_key().is_none(), "get_key should return None after clear");
    }
}

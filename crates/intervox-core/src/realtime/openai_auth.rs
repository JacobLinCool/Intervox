//! Pure classification of an OpenAI key-validation attempt. No network here:
//! the Tauri layer performs the HTTP request and passes the status (or None
//! on transport failure) to `classify_validation`.

pub const VALIDATION_URL: &str = "https://api.openai.com/v1/models";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KeyValidation {
    Verified,
    InvalidKey,
    Offline,
    Unknown,
}

pub fn classify_validation(http_status: Option<u16>) -> KeyValidation {
    match http_status {
        None => KeyValidation::Offline,
        Some(401) | Some(403) => KeyValidation::InvalidKey,
        Some(s) if (200..300).contains(&s) || s == 429 => KeyValidation::Verified,
        Some(_) => KeyValidation::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_statuses() {
        assert_eq!(classify_validation(Some(200)), KeyValidation::Verified);
        assert_eq!(classify_validation(Some(401)), KeyValidation::InvalidKey);
        assert_eq!(classify_validation(Some(403)), KeyValidation::InvalidKey);
        assert_eq!(classify_validation(Some(429)), KeyValidation::Verified);
        assert_eq!(classify_validation(Some(500)), KeyValidation::Unknown);
        assert_eq!(classify_validation(None), KeyValidation::Offline);
    }
}

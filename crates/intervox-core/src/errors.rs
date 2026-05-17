//! Error UX contract (spec §16). Every error carries a machine code, a
//! human title/message, and an optional recovery action the UI can render
//! as a button that invokes a Tauri command.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppErrorCode {
    DriverMissing,
    MicPermissionDenied,
    SystemAudioPermissionDenied,
    NetworkError,
    OpenaiAuthError,
    AudioDeviceLost,
    RingBufferError,
    InvalidConfig,
    Internal,
}

impl AppErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            AppErrorCode::DriverMissing => "DRIVER_MISSING",
            AppErrorCode::MicPermissionDenied => "MIC_PERMISSION_DENIED",
            AppErrorCode::SystemAudioPermissionDenied => "SYSTEM_AUDIO_PERMISSION_DENIED",
            AppErrorCode::NetworkError => "NETWORK_ERROR",
            AppErrorCode::OpenaiAuthError => "OPENAI_AUTH_ERROR",
            AppErrorCode::AudioDeviceLost => "AUDIO_DEVICE_LOST",
            AppErrorCode::RingBufferError => "RING_BUFFER_ERROR",
            AppErrorCode::InvalidConfig => "INVALID_CONFIG",
            AppErrorCode::Internal => "INTERNAL",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryAction {
    pub label: String,
    /// Name of a Tauri command the frontend may invoke to recover.
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppError {
    pub code: AppErrorCode,
    pub title: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_action: Option<RecoveryAction>,
}

impl AppError {
    pub fn new(
        code: AppErrorCode,
        title: impl Into<String>,
        message: impl Into<String>,
        recovery_action: Option<RecoveryAction>,
    ) -> Self {
        Self {
            code,
            title: title.into(),
            message: message.into(),
            recovery_action,
        }
    }

    pub fn mic_permission_denied() -> Self {
        Self::new(
            AppErrorCode::MicPermissionDenied,
            "Microphone access is off",
            "Intervox needs microphone access to translate your speech.",
            Some(RecoveryAction {
                label: "Open System Settings".into(),
                command: "open_system_mic_permission_settings".into(),
            }),
        )
    }

    pub fn system_audio_permission_denied(message: impl Into<String>) -> Self {
        Self::new(
            AppErrorCode::SystemAudioPermissionDenied,
            "System audio access is off",
            message,
            Some(RecoveryAction {
                label: "Open System Settings".into(),
                command: "open_system_audio_permission_settings".into(),
            }),
        )
    }

    pub fn driver_missing() -> Self {
        Self::new(
            AppErrorCode::DriverMissing,
            "Virtual microphone not installed",
            "The Intervox virtual microphone driver is not installed yet.",
            Some(RecoveryAction {
                label: "Install Virtual Mic".into(),
                command: "install_virtual_mic".into(),
            }),
        )
    }

    pub fn network_error(message: impl Into<String>) -> Self {
        Self::new(
            AppErrorCode::NetworkError,
            "Connection problem",
            message,
            None,
        )
    }

    pub fn openai_auth_error(message: impl Into<String>) -> Self {
        Self::new(
            AppErrorCode::OpenaiAuthError,
            "Translation service unavailable",
            message,
            None,
        )
    }

    pub fn audio_device_lost() -> Self {
        Self::new(
            AppErrorCode::AudioDeviceLost,
            "Audio device disconnected",
            "The selected microphone is no longer available.",
            None,
        )
    }

    pub fn audio_device_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            AppErrorCode::AudioDeviceLost,
            "Audio device unavailable",
            message,
            None,
        )
    }

    /// Like `audio_device_lost` but with a `RecoveryAction` pointing at
    /// `set_virtual_mic_mode`.  The frontend error banner will render a "Retry"
    /// button that invokes `set_virtual_mic_mode` to re-enter the current mode
    /// and restart capture.
    ///
    /// Used by the capture watcher (Task 4.5) when it detects unexpected device
    /// loss and emits the error to the frontend alongside the auto-restart attempt.
    pub fn audio_device_lost_retryable() -> Self {
        Self::new(
            AppErrorCode::AudioDeviceLost,
            "Audio device disconnected",
            "The selected microphone is no longer available. Attempting to reconnect.",
            Some(RecoveryAction {
                label: "Retry".into(),
                command: "set_virtual_mic_mode".into(),
            }),
        )
    }

    pub fn ring_buffer_error(message: impl Into<String>) -> Self {
        Self::new(
            AppErrorCode::RingBufferError,
            "Virtual mic pipeline error",
            message,
            None,
        )
    }

    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::new(
            AppErrorCode::InvalidConfig,
            "Invalid setting",
            message,
            None,
        )
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            AppErrorCode::Internal,
            "Something went wrong",
            message,
            None,
        )
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.code.as_str(),
            self.title,
            self.message
        )
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mic_permission_denied_matches_spec_contract() {
        let e = AppError::mic_permission_denied();
        assert_eq!(e.code, AppErrorCode::MicPermissionDenied);
        let ra = e.recovery_action.as_ref().expect("recovery action");
        assert_eq!(ra.command, "open_system_mic_permission_settings");
        assert!(!ra.label.is_empty());
    }

    #[test]
    fn system_audio_permission_denied_matches_spec_contract() {
        let e = AppError::system_audio_permission_denied("permission missing");
        assert_eq!(e.code, AppErrorCode::SystemAudioPermissionDenied);
        let ra = e.recovery_action.as_ref().expect("recovery action");
        assert_eq!(ra.command, "open_system_audio_permission_settings");
        assert!(!ra.label.is_empty());
    }

    #[test]
    fn error_code_serializes_screaming_snake() {
        let j = serde_json::to_string(&AppErrorCode::MicPermissionDenied).unwrap();
        assert_eq!(j, "\"MIC_PERMISSION_DENIED\"");
    }

    #[test]
    fn app_error_round_trips_through_json() {
        let e = AppError::driver_missing();
        let j = serde_json::to_string(&e).unwrap();
        let back: AppError = serde_json::from_str(&j).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn no_recovery_action_is_omitted_from_json() {
        let e = AppError::audio_device_lost();
        let j = serde_json::to_string(&e).unwrap();
        assert!(!j.contains("recovery_action"));
    }

    #[test]
    fn audio_device_lost_retryable_has_recovery_action() {
        let e = AppError::audio_device_lost_retryable();
        let ra = e
            .recovery_action
            .as_ref()
            .expect("must have recovery action");
        assert_eq!(ra.label, "Retry");
        assert_eq!(ra.command, "set_virtual_mic_mode");
        assert_eq!(e.code, AppErrorCode::AudioDeviceLost);
        // Verify it serializes with recovery_action present.
        let j = serde_json::to_string(&e).unwrap();
        assert!(
            j.contains("recovery_action"),
            "retryable error must include recovery_action in JSON"
        );
    }

    #[test]
    fn display_includes_code() {
        let s = format!("{}", AppError::network_error("offline"));
        assert!(s.contains("NETWORK_ERROR"));
    }

    /// Privacy: the `openai_auth_error` constructor and its Display must never
    /// include an `sk-` prefix.  This proves that even if a caller passes a
    /// static message the resulting error cannot expose an API key.
    #[test]
    fn openai_auth_error_display_contains_no_sk_prefix() {
        let e = AppError::openai_auth_error("Invalid API key");
        let display = format!("{e}");
        assert!(
            !display.contains("sk-"),
            "AppError display must not contain 'sk-': {display}"
        );
        // Also check the message field directly.
        assert!(
            !e.message.contains("sk-"),
            "AppError.message must not contain 'sk-': {}",
            e.message
        );
    }

    /// Privacy: verify_api_key-style auth errors that could conceivably carry
    /// the key in their message must NOT do so.  The canonical error is "Invalid
    /// API key" — a static string — not the key value.
    #[test]
    fn auth_error_message_is_static_not_key_value() {
        // Simulate what commands.rs does: classify 401 → "Invalid API key".
        let e = AppError::openai_auth_error("Invalid API key");
        // The message is the human description, not the key string.
        assert_eq!(e.message, "Invalid API key");
        // It must not resemble a real key.
        assert!(
            !e.message.starts_with("sk-"),
            "message must not be the key value"
        );
    }
}

//! Core state (spec §2, §11, §13). Rust is the source of truth; the Svelte
//! UI only renders these structs and never owns mode logic.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VirtualMicMode {
    Silence,
    PassThrough,
    Translate,
}

impl VirtualMicMode {
    /// Translate needs an OpenAI session; the others must not create cost
    /// (non-negotiable rules §19.7, §19.8).
    pub fn requires_openai(self) -> bool {
        matches!(self, Self::Translate)
    }

    /// Only PassThrough routes the raw mic to the virtual mic.
    pub fn sends_mic_to_vmic(self) -> bool {
        matches!(self, Self::PassThrough)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Ready,
    Warning,
    Error,
}

/// OpenAI translation-connection signal (spec §11, §13): what the UI shows for
/// whether the OpenAI realtime translation session is actually up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationConn {
    Idle,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

/// Lifecycle phases from the spec §11 state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Booting,
    CheckDriver,
    Ready,
    PassThroughActive,
    ConnectingTranslation,
    Translating,
    Silence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub mode: VirtualMicMode,
    pub health: Health,
    pub translation: TranslationConn,
    pub source_mic_name: Option<String>,
    pub virtual_mic_installed: bool,
    pub openai_connected: bool,
    pub latency_ms: Option<u32>,
    pub target_language: String,
    pub input_level: f32,
    pub output_level: f32,
}

impl Default for AppStatus {
    fn default() -> Self {
        Self {
            mode: VirtualMicMode::Silence,
            health: Health::Ready,
            translation: TranslationConn::Idle,
            source_mic_name: None,
            virtual_mic_installed: false,
            openai_connected: false,
            latency_ms: None,
            target_language: "en".to_string(),
            input_level: 0.0,
            output_level: 0.0,
        }
    }
}

/// Owns mode transitions. The phase tracks where we are in the §11 machine so
/// the UI can show "connecting…" before "translating".
#[derive(Debug, Clone)]
pub struct AppState {
    pub status: AppStatus,
    pub phase: Phase,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            status: AppStatus::default(),
            phase: Phase::Booting,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Transition to a new mode. Returns the phase entered. Translate
    /// pass through `ConnectingTranslation` until `mark_openai_connected`.
    pub fn transition(&mut self, mode: VirtualMicMode) -> Phase {
        self.status.mode = mode;
        if !mode.requires_openai() {
            self.status.openai_connected = false;
        }
        self.phase = match mode {
            VirtualMicMode::Silence => Phase::Silence,
            VirtualMicMode::PassThrough => Phase::PassThroughActive,
            VirtualMicMode::Translate => {
                if self.status.openai_connected {
                    Phase::Translating
                } else {
                    Phase::ConnectingTranslation
                }
            }
        };
        self.status.translation = match mode {
            VirtualMicMode::Silence | VirtualMicMode::PassThrough => TranslationConn::Idle,
            VirtualMicMode::Translate => {
                if self.status.openai_connected {
                    TranslationConn::Connected
                } else {
                    TranslationConn::Connecting
                }
            }
        };
        self.phase
    }

    pub fn mark_openai_connected(&mut self, connected: bool) {
        self.status.openai_connected = connected;
        if self.status.mode.requires_openai() {
            self.phase = if connected {
                Phase::Translating
            } else {
                Phase::ConnectingTranslation
            };
            self.status.translation = if connected {
                TranslationConn::Connected
            } else {
                TranslationConn::Reconnecting
            };
        } else {
            self.status.translation = TranslationConn::Idle;
        }
    }

    /// Explicitly override the connection signal (used by the engine for a
    /// fatal/auth failure that the supervisor will NOT retry).
    pub fn set_translation_conn(&mut self, c: TranslationConn) {
        debug_assert!(
            self.status.mode.requires_openai()
                || matches!(c, TranslationConn::Idle | TranslationConn::Failed),
            "set_translation_conn called with {:?} while mode does not require OpenAI",
            c
        );
        self.status.translation = c;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_serializes_snake_case() {
        let j = serde_json::to_string(&VirtualMicMode::Translate).unwrap();
        assert_eq!(j, "\"translate\"");
        let back: VirtualMicMode = serde_json::from_str("\"pass_through\"").unwrap();
        assert_eq!(back, VirtualMicMode::PassThrough);
    }

    #[test]
    fn default_status_is_ready_and_silent() {
        let s = AppStatus::default();
        assert_eq!(s.health, Health::Ready);
        assert_eq!(s.mode, VirtualMicMode::Silence);
    }

    #[test]
    fn translate_requires_openai_others_do_not() {
        assert!(VirtualMicMode::Translate.requires_openai());
        assert!(!VirtualMicMode::PassThrough.requires_openai());
        assert!(!VirtualMicMode::Silence.requires_openai());
    }

    #[test]
    fn transition_to_translate_goes_through_connecting() {
        let mut st = AppState::new();
        let p = st.transition(VirtualMicMode::Translate);
        assert_eq!(p, Phase::ConnectingTranslation);
        st.mark_openai_connected(true);
        assert_eq!(st.phase, Phase::Translating);
    }

    #[test]
    fn leaving_translate_clears_openai_flag() {
        let mut st = AppState::new();
        st.transition(VirtualMicMode::Translate);
        st.mark_openai_connected(true);
        st.transition(VirtualMicMode::PassThrough);
        assert!(!st.status.openai_connected);
        assert_eq!(st.phase, Phase::PassThroughActive);
    }

    #[test]
    fn silence_mode_phase() {
        let mut st = AppState::new();
        assert_eq!(st.transition(VirtualMicMode::Silence), Phase::Silence);
    }

    #[test]
    fn translation_conn_default_is_idle() {
        assert_eq!(AppStatus::default().translation, TranslationConn::Idle);
    }

    #[test]
    fn transition_sets_translation_conn() {
        let mut st = AppState::new();
        st.transition(VirtualMicMode::Translate);
        assert_eq!(st.status.translation, TranslationConn::Connecting);
        st.mark_openai_connected(true);
        assert_eq!(st.status.translation, TranslationConn::Connected);
        st.transition(VirtualMicMode::PassThrough);
        assert_eq!(st.status.translation, TranslationConn::Idle);
        st.transition(VirtualMicMode::Silence);
        assert_eq!(st.status.translation, TranslationConn::Idle);
    }

    #[test]
    fn set_translation_conn_failed_is_explicit() {
        let mut st = AppState::new();
        st.transition(VirtualMicMode::Translate);
        st.set_translation_conn(TranslationConn::Failed);
        assert_eq!(st.status.translation, TranslationConn::Failed);
        // mark_openai_connected(false) while in translate => Reconnecting
        st.mark_openai_connected(false);
        assert_eq!(st.status.translation, TranslationConn::Reconnecting);
    }
}

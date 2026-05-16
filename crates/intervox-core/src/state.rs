//! Core state (spec §2, §11, §13). Rust is the source of truth; the Svelte
//! UI only renders these structs and never owns mode logic.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VirtualMicMode {
    Silence,
    PassThrough,
    Translate,
    TranslateWithOriginal,
}

impl VirtualMicMode {
    /// Translate modes need an OpenAI session; the others must not create cost
    /// (non-negotiable rules §19.7, §19.8).
    pub fn requires_openai(self) -> bool {
        matches!(self, Self::Translate | Self::TranslateWithOriginal)
    }

    /// Only PassThrough routes the raw mic to the virtual mic.
    pub fn sends_mic_to_vmic(self) -> bool {
        matches!(self, Self::PassThrough)
    }

    /// Original mic audio only reaches the virtual mic in TranslateWithOriginal
    /// (non-negotiable rule §19.9 — never leak original otherwise).
    pub fn sends_original_audio(self) -> bool {
        matches!(self, Self::TranslateWithOriginal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Ready,
    Warning,
    Error,
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
    TranslatingWithOriginal,
    Silence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub mode: VirtualMicMode,
    pub health: Health,
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

    /// Transition to a new mode. Returns the phase entered. Translate modes
    /// pass through `ConnectingTranslation` until `mark_openai_connected`.
    pub fn transition(&mut self, mode: VirtualMicMode) -> Phase {
        self.status.mode = mode;
        if !mode.requires_openai() {
            self.status.openai_connected = false;
        }
        self.phase = match mode {
            VirtualMicMode::Silence => Phase::Silence,
            VirtualMicMode::PassThrough => Phase::PassThroughActive,
            VirtualMicMode::Translate | VirtualMicMode::TranslateWithOriginal => {
                if self.status.openai_connected {
                    self.translating_phase()
                } else {
                    Phase::ConnectingTranslation
                }
            }
        };
        self.phase
    }

    pub fn mark_openai_connected(&mut self, connected: bool) {
        self.status.openai_connected = connected;
        if self.status.mode.requires_openai() {
            self.phase = if connected {
                self.translating_phase()
            } else {
                Phase::ConnectingTranslation
            };
        }
    }

    fn translating_phase(&self) -> Phase {
        match self.status.mode {
            VirtualMicMode::TranslateWithOriginal => Phase::TranslatingWithOriginal,
            _ => Phase::Translating,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_serializes_snake_case() {
        let j = serde_json::to_string(&VirtualMicMode::TranslateWithOriginal).unwrap();
        assert_eq!(j, "\"translate_with_original\"");
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
        assert!(VirtualMicMode::TranslateWithOriginal.requires_openai());
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
    fn transition_to_translate_with_original_when_connected() {
        let mut st = AppState::new();
        st.mark_openai_connected(true);
        let p = st.transition(VirtualMicMode::TranslateWithOriginal);
        assert_eq!(p, Phase::TranslatingWithOriginal);
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
}

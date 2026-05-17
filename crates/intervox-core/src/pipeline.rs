//! Pure mode → routing decision (spec §7, §17, §19). No audio is processed
//! here; this encodes *what* each mode is allowed to do so the non-negotiable
//! engineering rules are testable in isolation and impossible to violate by
//! accident in the audio thread.

use crate::state::VirtualMicMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDecision {
    /// Capture the source mic at all.
    pub capture_mic: bool,
    /// Maintain an OpenAI translation session (and therefore incur cost).
    pub openai_connected: bool,
    /// Stream captured mic audio to OpenAI.
    pub mic_to_openai: bool,
    /// Write raw (resampled) mic audio straight to the virtual mic.
    pub mic_to_vmic: bool,
    /// Write translated audio to the virtual mic.
    pub translated_to_vmic: bool,
    /// Mix delayed original under the translation.
    pub mix_original: bool,
    /// Virtual mic should emit silence.
    pub vmic_silence: bool,
    /// Captions should be updating.
    pub captions_active: bool,
}

pub fn route(mode: VirtualMicMode, original_voice_percent: u32) -> RouteDecision {
    match mode {
        VirtualMicMode::Silence => RouteDecision {
            capture_mic: false,
            openai_connected: false,
            mic_to_openai: false,
            mic_to_vmic: false,
            translated_to_vmic: false,
            mix_original: false,
            vmic_silence: true,
            captions_active: false,
        },
        VirtualMicMode::PassThrough => RouteDecision {
            capture_mic: true,
            openai_connected: false,
            mic_to_openai: false,
            mic_to_vmic: true,
            translated_to_vmic: false,
            mix_original: false,
            vmic_silence: false,
            captions_active: false,
        },
        VirtualMicMode::Translate => RouteDecision {
            capture_mic: true,
            openai_connected: true,
            mic_to_openai: true,
            mic_to_vmic: false,
            translated_to_vmic: true,
            mix_original: original_voice_percent > 0,
            vmic_silence: false,
            captions_active: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_sends_no_audio_no_openai_rule_19_7() {
        let r = route(VirtualMicMode::Silence, 15);
        assert!(r.vmic_silence);
        assert!(!r.openai_connected);
        assert!(!r.mic_to_vmic && !r.translated_to_vmic);
        assert!(!r.captions_active);
    }

    #[test]
    fn passthrough_no_openai_cost_rule_19_8() {
        let r = route(VirtualMicMode::PassThrough, 15);
        assert!(r.mic_to_vmic);
        assert!(!r.openai_connected && !r.mic_to_openai);
    }

    #[test]
    fn translate_zero_original_does_not_leak_original_rule_19_9() {
        let r = route(VirtualMicMode::Translate, 0);
        assert!(r.mic_to_openai && r.translated_to_vmic);
        assert!(!r.mic_to_vmic, "raw mic must NOT reach vmic in Translate");
        assert!(!r.mix_original);
        assert!(r.captions_active);
    }

    #[test]
    fn translate_positive_original_percent_mixes() {
        let r = route(VirtualMicMode::Translate, 15);
        assert!(r.translated_to_vmic && r.mix_original);
        assert!(r.openai_connected);
    }

    #[test]
    fn only_translate_uses_openai() {
        for m in [VirtualMicMode::Silence, VirtualMicMode::PassThrough] {
            assert!(!route(m, 15).openai_connected);
        }
        assert!(route(VirtualMicMode::Translate, 0).openai_connected);
        assert!(route(VirtualMicMode::Translate, 15).openai_connected);
    }
}

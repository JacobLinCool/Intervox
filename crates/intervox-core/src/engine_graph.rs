//! Typed per-frame routing for the live audio engine. This is a thin
//! projection of `pipeline::route` so the engine never re-encodes the
//! no-cost / no-leak rules — those live in `pipeline` and are tested there.

use crate::pipeline::route;
use crate::state::VirtualMicMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRouting {
    pub mic_to_ring: bool,
    pub mic_to_openai: bool,
    pub openai_connected: bool,
    pub translated_to_ring: bool,
    pub mix_original: bool,
    pub ring_silence: bool,
}

impl FrameRouting {
    pub fn for_mode(mode: VirtualMicMode) -> Self {
        let r = route(mode);
        Self {
            mic_to_ring: r.mic_to_vmic,
            mic_to_openai: r.mic_to_openai,
            openai_connected: r.openai_connected,
            translated_to_ring: r.translated_to_vmic,
            mix_original: r.mix_original,
            ring_silence: r.vmic_silence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::VirtualMicMode::*;

    #[test]
    fn passthrough_sends_mic_to_ring_only() {
        let g = FrameRouting::for_mode(PassThrough);
        assert!(g.mic_to_ring && !g.mic_to_openai && !g.translated_to_ring && !g.ring_silence);
    }

    #[test]
    fn silence_is_silent_and_offline() {
        let g = FrameRouting::for_mode(Silence);
        assert!(g.ring_silence && !g.mic_to_ring && !g.openai_connected);
    }

    #[test]
    fn translate_never_leaks_mic_to_ring() {
        for m in [Translate, TranslateWithOriginal] {
            assert!(
                !FrameRouting::for_mode(m).mic_to_ring,
                "raw mic must not reach ring"
            );
        }
    }
}

//! Gain, ducking, mixing and limiting (spec §2.4, §7.4). All level math is in
//! dB internally; the UI converts to percentages elsewhere.

use serde::{Deserialize, Serialize};

/// Output ceiling, just under full-scale to leave headroom and avoid the
/// meeting app seeing hard 0 dBFS clipping.
pub const LIMITER_CEILING: f32 = 0.997;

pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn apply_gain(samples: &mut [f32], gain_lin: f32) {
    for s in samples.iter_mut() {
        *s *= gain_lin;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MixSettings {
    pub original_gain_db: f32,
    pub translated_gain_db: f32,
    pub duck_original: bool,
    pub limiter_enabled: bool,
}

impl Default for MixSettings {
    fn default() -> Self {
        Self {
            original_gain_db: -18.0,
            translated_gain_db: 0.0,
            duck_original: true,
            limiter_enabled: true,
        }
    }
}

/// Extra attenuation applied to the original voice while translated speech is
/// present, so the translation stays clearly primary (spec §2.4 ducking).
const DUCK_ATTEN_DB: f32 = -12.0;
const DUCK_TRIGGER: f32 = 0.02;

#[derive(Debug, Clone, Copy)]
pub struct Limiter {
    pub ceiling: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        Self {
            ceiling: LIMITER_CEILING,
        }
    }
}

impl Limiter {
    pub fn process(&self, samples: &mut [f32]) {
        for s in samples.iter_mut() {
            *s = s.clamp(-self.ceiling, self.ceiling);
        }
    }
}

fn frame_peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, &v| m.max(v.abs()))
}

/// Mix translated (primary) with original (background). `original` may be
/// shorter/longer; mixing stops at the shorter length.
pub fn mix_frames(translated: &[f32], original: &[f32], settings: &MixSettings) -> Vec<f32> {
    let n = translated.len().max(original.len());
    let tgain = db_to_lin(settings.translated_gain_db);
    let mut ogain = db_to_lin(settings.original_gain_db);

    if settings.duck_original && frame_peak(translated) * tgain > DUCK_TRIGGER {
        ogain *= db_to_lin(DUCK_ATTEN_DB);
    }

    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = translated.get(i).copied().unwrap_or(0.0) * tgain;
        let o = original.get(i).copied().unwrap_or(0.0) * ogain;
        out.push(t + o);
    }
    if settings.limiter_enabled {
        Limiter::default().process(&mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_to_lin_reference_points() {
        assert!((db_to_lin(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_lin(-6.0) - 0.5012).abs() < 1e-3);
        assert!((db_to_lin(-18.0) - 0.1259).abs() < 1e-3);
    }

    #[test]
    fn apply_gain_scales() {
        let mut s = vec![1.0, -1.0, 0.5];
        apply_gain(&mut s, 0.5);
        assert_eq!(s, vec![0.5, -0.5, 0.25]);
    }

    #[test]
    fn original_at_minus_18db_is_quieter_than_translated() {
        let full = vec![1.0f32; 256];
        let zeros = vec![0.0f32; 256];
        let s = MixSettings::default();
        let only_original = mix_frames(&zeros, &full, &s);
        let only_translated = mix_frames(&full, &zeros, &s);
        assert!(frame_peak(&only_original) < frame_peak(&only_translated));
        assert!(frame_peak(&only_original) < 0.2);
    }

    #[test]
    fn limiter_never_exceeds_full_scale() {
        let loud = vec![5.0f32, -5.0, 2.0, -2.0];
        let out = mix_frames(&loud, &[], &MixSettings::default());
        for v in &out {
            assert!(v.abs() <= LIMITER_CEILING + 1e-6);
            assert!(v.abs() <= 1.0);
        }
    }

    #[test]
    fn duck_reduces_original_when_translated_active() {
        let full = vec![1.0f32; 256];
        let mut s = MixSettings {
            limiter_enabled: false,
            ..MixSettings::default()
        };

        s.duck_original = true;
        let ducked = mix_frames(&full, &full, &s);
        s.duck_original = false;
        let unducked = mix_frames(&full, &full, &s);

        // With ducking, original contributes less, so total peak is lower.
        assert!(frame_peak(&ducked) < frame_peak(&unducked));
    }
}

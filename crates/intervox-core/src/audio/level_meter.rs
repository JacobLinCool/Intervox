//! Input/output level metering for the UI VU meters (spec §4.2 InputLevel /
//! OutputLevel events).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AudioLevel {
    pub peak: f32,
    pub rms: f32,
}

#[derive(Debug, Default)]
pub struct LevelMeter;

impl LevelMeter {
    pub fn measure(samples: &[f32]) -> AudioLevel {
        if samples.is_empty() {
            return AudioLevel {
                peak: 0.0,
                rms: 0.0,
            };
        }
        let mut peak = 0.0f32;
        let mut sumsq = 0.0f64;
        for &s in samples {
            peak = peak.max(s.abs());
            sumsq += (s as f64) * (s as f64);
        }
        let rms = (sumsq / samples.len() as f64).sqrt() as f32;
        AudioLevel { peak, rms }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_zero() {
        let l = LevelMeter::measure(&[0.0; 128]);
        assert_eq!(l.peak, 0.0);
        assert_eq!(l.rms, 0.0);
    }

    #[test]
    fn empty_is_zero() {
        let l = LevelMeter::measure(&[]);
        assert_eq!(l.peak, 0.0);
    }

    #[test]
    fn full_scale_sine_rms_is_root_half() {
        let s: Vec<f32> = (0..4800)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin())
            .collect();
        let l = LevelMeter::measure(&s);
        assert!((l.peak - 1.0).abs() < 0.01, "peak {}", l.peak);
        assert!((l.rms - 0.707).abs() < 0.01, "rms {}", l.rms);
    }

    #[test]
    fn serde_shape() {
        let j = serde_json::to_string(&AudioLevel {
            peak: 0.5,
            rms: 0.25,
        })
        .unwrap();
        assert_eq!(j, r#"{"peak":0.5,"rms":0.25}"#);
    }
}

//! Energy voice-activity detector with attack/hangover hysteresis. Used to
//! gate OpenAI sends and to drive ducking.

#[derive(Debug, Clone)]
pub struct Vad {
    threshold_rms: f32,
    attack_frames: u32,
    hangover_frames: u32,
    above: u32,
    silence: u32,
    speaking: bool,
}

impl Default for Vad {
    fn default() -> Self {
        Self::new(0.01, 2, 12)
    }
}

impl Vad {
    pub fn new(threshold_rms: f32, attack_frames: u32, hangover_frames: u32) -> Self {
        Self {
            threshold_rms,
            attack_frames,
            hangover_frames,
            above: 0,
            silence: 0,
            speaking: false,
        }
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking
    }

    /// Feed one frame; returns the (possibly unchanged) speaking state.
    pub fn push(&mut self, frame: &[f32]) -> bool {
        let rms = if frame.is_empty() {
            0.0
        } else {
            (frame.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / frame.len() as f64)
                .sqrt() as f32
        };

        if rms >= self.threshold_rms {
            self.above += 1;
            self.silence = 0;
            if self.above >= self.attack_frames {
                self.speaking = true;
            }
        } else {
            self.above = 0;
            self.silence += 1;
            if self.silence >= self.hangover_frames {
                self.speaking = false;
            }
        }
        self.speaking
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loud(n: usize) -> Vec<f32> {
        vec![0.5; n]
    }
    fn quiet(n: usize) -> Vec<f32> {
        vec![0.0; n]
    }

    #[test]
    fn silence_is_not_speaking() {
        let mut v = Vad::default();
        for _ in 0..20 {
            assert!(!v.push(&quiet(160)));
        }
    }

    #[test]
    fn speech_detected_after_attack() {
        let mut v = Vad::new(0.01, 3, 10);
        assert!(!v.push(&loud(160)));
        assert!(!v.push(&loud(160)));
        assert!(v.push(&loud(160)));
    }

    #[test]
    fn hangover_keeps_speaking_through_short_gap() {
        let mut v = Vad::new(0.01, 1, 5);
        v.push(&loud(160));
        assert!(v.is_speaking());
        for _ in 0..4 {
            assert!(v.push(&quiet(160)), "should still be speaking during hangover");
        }
        assert!(!v.push(&quiet(160)), "should stop after hangover expires");
    }
}

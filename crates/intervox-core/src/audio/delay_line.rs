//! Delay line for TranslateWithOriginal (spec §7.4). The original voice must
//! be delayed so it roughly aligns with the much later translated audio,
//! otherwise the original races ahead of the translation.

/// original_delay_ms = translation_latency_ms - 100, clamped to [300, 2500].
pub fn compute_original_delay_ms(translation_latency_ms: u32) -> u32 {
    let raw = translation_latency_ms.saturating_sub(100);
    raw.clamp(300, 2500)
}

pub struct DelayLine {
    buf: Vec<f32>,
    delay_samples: usize,
    pos: usize,
    filled: usize,
}

impl DelayLine {
    pub fn new(sample_rate: u32, delay_ms: u32) -> Self {
        let delay_samples = (sample_rate as u64 * delay_ms as u64 / 1000) as usize;
        Self {
            buf: vec![0.0; delay_samples.max(1)],
            delay_samples,
            pos: 0,
            filled: 0,
        }
    }

    pub fn delay_samples(&self) -> usize {
        self.delay_samples
    }

    /// Push input, return the delayed output (same length). First
    /// `delay_samples` outputs are zeros.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.delay_samples == 0 {
            return input.to_vec();
        }
        let mut out = Vec::with_capacity(input.len());
        for &x in input {
            let delayed = if self.filled >= self.delay_samples {
                self.buf[self.pos]
            } else {
                self.filled += 1;
                0.0
            };
            self.buf[self.pos] = x;
            self.pos = (self.pos + 1) % self.buf.len();
            out.push(delayed);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_formula_matches_spec() {
        assert_eq!(compute_original_delay_ms(100), 300); // below min
        assert_eq!(compute_original_delay_ms(900), 800);
        assert_eq!(compute_original_delay_ms(10_000), 2500); // above max
        assert_eq!(compute_original_delay_ms(0), 300);
    }

    #[test]
    fn first_n_outputs_are_zero_then_original() {
        let mut d = DelayLine::new(48000, 1); // 48 samples
        let input: Vec<f32> = (1..=200).map(|i| i as f32).collect();
        let out = d.process(&input);
        assert_eq!(out.len(), 200);
        assert!(out[..48].iter().all(|&v| v == 0.0));
        assert_eq!(out[48], 1.0);
        assert_eq!(out[49], 2.0);
    }

    #[test]
    fn zero_delay_is_passthrough() {
        let mut d = DelayLine::new(48000, 0);
        assert_eq!(d.process(&[1.0, 2.0, 3.0]), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn streaming_across_chunks_preserves_alignment() {
        let mut d = DelayLine::new(1000, 2); // 2-sample delay
        let mut out = Vec::new();
        out.extend(d.process(&[1.0]));
        out.extend(d.process(&[2.0, 3.0]));
        out.extend(d.process(&[4.0, 5.0]));
        assert_eq!(out, vec![0.0, 0.0, 1.0, 2.0, 3.0]);
    }
}

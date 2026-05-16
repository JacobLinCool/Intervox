//! Linear-interpolation resampler. Source mics arrive at 44.1/48 kHz; OpenAI
//! wants 24 kHz; the virtual mic wants 48 kHz (spec §6). Streaming-safe: phase
//! and a one-sample history carry across chunk boundaries.

pub fn resample(input: &[f32], in_hz: u32, out_hz: u32) -> Vec<f32> {
    let mut r = LinearResampler::new(in_hz, out_hz);
    r.process(input)
}

pub struct LinearResampler {
    /// Input samples consumed per output sample.
    step: f64,
    /// Fractional read position. Index 0 refers to `prev` once `has_prev`.
    cursor: f64,
    prev: f32,
    has_prev: bool,
    pub in_hz: u32,
    pub out_hz: u32,
}

impl LinearResampler {
    pub fn new(in_hz: u32, out_hz: u32) -> Self {
        Self {
            step: in_hz as f64 / out_hz as f64,
            cursor: 0.0,
            prev: 0.0,
            has_prev: false,
            in_hz,
            out_hz,
        }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        if self.in_hz == self.out_hz {
            return input.to_vec();
        }

        // Extended buffer: [prev?, input...]. prev provides index 0 continuity.
        let mut ext: Vec<f32> = Vec::with_capacity(input.len() + 1);
        if self.has_prev {
            ext.push(self.prev);
        }
        ext.extend_from_slice(input);

        let mut out = Vec::new();
        let last = ext.len() - 1;
        while self.cursor < last as f64 {
            let i = self.cursor.floor() as usize;
            let frac = (self.cursor - i as f64) as f32;
            let a = ext[i];
            let b = ext[i + 1];
            out.push(a + (b - a) * frac);
            self.cursor += self.step;
        }

        // The last ext sample becomes index 0 for the next call.
        self.prev = ext[last];
        self.has_prev = true;
        self.cursor -= last as f64;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, sr: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin())
            .collect()
    }

    fn zero_crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    #[test]
    fn identity_when_rates_equal() {
        let s = sine(1000.0, 48000, 480);
        assert_eq!(resample(&s, 48000, 48000), s);
    }

    #[test]
    fn downsample_48k_to_24k_halves_count() {
        let s = sine(1000.0, 48000, 4800);
        let out = resample(&s, 48000, 24000);
        assert!((out.len() as i64 - 2400).abs() <= 1, "len {}", out.len());
    }

    #[test]
    fn downsample_preserves_frequency() {
        // 1 kHz over 1 s -> ~1000 positive-going zero crossings at any rate.
        let s = sine(1000.0, 48000, 48000);
        let out = resample(&s, 48000, 24000);
        let zc = zero_crossings(&out);
        assert!((zc as i64 - 1000).abs() <= 20, "zc {zc}");
    }

    #[test]
    fn upsample_24k_to_48k_doubles_count() {
        let s = sine(1000.0, 24000, 2400);
        let out = resample(&s, 24000, 48000);
        assert!((out.len() as i64 - 4800).abs() <= 2, "len {}", out.len());
    }

    #[test]
    fn ratio_44100_to_24000() {
        let s = sine(440.0, 44100, 44100);
        let out = resample(&s, 44100, 24000);
        let expected = 44100.0 * 24000.0 / 44100.0;
        assert!(
            (out.len() as f64 - expected).abs() < 3.0,
            "len {}",
            out.len()
        );
    }

    #[test]
    fn dc_signal_stays_dc() {
        let dc = vec![0.5f32; 4800];
        let out = resample(&dc, 48000, 24000);
        for v in &out[2..out.len() - 2] {
            assert!((v - 0.5).abs() < 1e-4, "got {v}");
        }
    }

    #[test]
    fn streaming_matches_oneshot_count() {
        let s = sine(1000.0, 48000, 9600);
        let oneshot = resample(&s, 48000, 24000);
        let mut r = LinearResampler::new(48000, 24000);
        let mut streamed = Vec::new();
        for chunk in s.chunks(512) {
            streamed.extend(r.process(chunk));
        }
        assert!((streamed.len() as i64 - oneshot.len() as i64).abs() <= 2);
    }
}

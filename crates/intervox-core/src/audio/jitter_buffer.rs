//! Jitter buffer for OpenAI translated audio (spec §7.3). Absorbs network
//! bursts: withholds output until a target fill is reached, then yields a
//! steady stream. Underrun yields silence (the virtual mic must never block).

use std::collections::VecDeque;

pub struct JitterBuffer {
    queue: VecDeque<f32>,
    sample_rate: u32,
    target_frames: usize,
    max_frames: usize,
    primed: bool,
}

impl JitterBuffer {
    pub fn new(sample_rate: u32, target_ms: u32, max_ms: u32) -> Self {
        let f = |ms: u32| (sample_rate as u64 * ms as u64 / 1000) as usize;
        Self {
            queue: VecDeque::new(),
            sample_rate,
            target_frames: f(target_ms),
            max_frames: f(max_ms),
            primed: false,
        }
    }

    pub fn buffered_ms(&self) -> u32 {
        (self.queue.len() as u64 * 1000 / self.sample_rate.max(1) as u64) as u32
    }

    pub fn push(&mut self, samples: &[f32]) {
        self.queue.extend(samples.iter().copied());
        // Overrun: drop oldest to bound latency.
        while self.queue.len() > self.max_frames {
            self.queue.pop_front();
        }
        if self.queue.len() >= self.target_frames {
            self.primed = true;
        }
    }

    /// Pull exactly `n` frames. Returns silence (zeros) while not yet primed
    /// or on underrun — never blocks, never panics.
    pub fn pull(&mut self, n: usize) -> Vec<f32> {
        if !self.primed {
            return vec![0.0; n];
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            match self.queue.pop_front() {
                Some(s) => out.push(s),
                None => {
                    out.push(0.0);
                    self.primed = false; // re-prime after underrun
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn withholds_until_primed_then_yields_data() {
        let mut jb = JitterBuffer::new(48000, 100, 500); // target 4800 frames
        jb.push(&vec![0.5; 1000]);
        assert!(jb.pull(480).iter().all(|&s| s == 0.0), "not primed yet");
        jb.push(&vec![0.5; 5000]);
        let out = jb.pull(480);
        assert!(out.iter().all(|&s| s == 0.5), "primed -> real data");
    }

    #[test]
    fn underrun_yields_silence_not_panic() {
        let mut jb = JitterBuffer::new(48000, 10, 500);
        jb.push(&vec![0.5; 600]);
        let out = jb.pull(100000);
        assert_eq!(out.len(), 100000);
        assert_eq!(*out.last().unwrap(), 0.0);
    }

    #[test]
    fn overrun_drops_oldest() {
        let mut jb = JitterBuffer::new(48000, 10, 100); // max 4800 frames
        jb.push(&vec![0.1; 10000]);
        assert!(jb.buffered_ms() <= 100);
    }

    #[test]
    fn reports_buffered_ms() {
        let mut jb = JitterBuffer::new(48000, 10, 1000);
        jb.push(&vec![0.0; 48000]);
        assert_eq!(jb.buffered_ms(), 1000);
    }
}

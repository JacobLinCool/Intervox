//! Usage accounting. Counts uplink audio actually sent to OpenAI (24 kHz mono
//! PCM16) and converts to minutes and an estimated cost. NOT the OpenAI bill —
//! a local estimate at a flat per-minute rate.

use serde::{Deserialize, Serialize};

pub const SAMPLE_RATE_HZ: f64 = 24_000.0;
pub const COST_PER_MINUTE_USD: f64 = 0.034;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageStore {
    /// UTC year-month, "YYYY-MM".
    pub month: String,
    pub month_seconds: f64,
    pub total_seconds: f64,
}

impl Default for UsageStore {
    fn default() -> Self {
        Self {
            month: String::new(),
            month_seconds: 0.0,
            total_seconds: 0.0,
        }
    }
}

impl UsageStore {
    /// Add `n` PCM16 samples sent at 24 kHz. `now_month` is the current UTC
    /// "YYYY-MM"; a change rolls the monthly bucket to zero (total untouched).
    pub fn add_samples(&mut self, n: u64, now_month: &str) {
        if self.month != now_month {
            self.month = now_month.to_string();
            self.month_seconds = 0.0;
        }
        let secs = n as f64 / SAMPLE_RATE_HZ;
        self.month_seconds += secs;
        self.total_seconds += secs;
    }

    pub fn month_minutes(&self) -> f64 {
        self.month_seconds / 60.0
    }
    pub fn total_minutes(&self) -> f64 {
        self.total_seconds / 60.0
    }
    pub fn month_usd(&self) -> f64 {
        self.month_minutes() * COST_PER_MINUTE_USD
    }
    pub fn total_usd(&self) -> f64 {
        self.total_minutes() * COST_PER_MINUTE_USD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_convert_to_seconds_at_24k() {
        let mut u = UsageStore::default();
        u.add_samples(24_000, "2026-05");
        assert!((u.total_seconds - 1.0).abs() < 1e-9);
        assert!((u.month_seconds - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cost_is_per_minute_rate() {
        let mut u = UsageStore::default();
        u.add_samples(24_000 * 60, "2026-05"); // 60 s = 1 min
        assert!((u.total_usd() - 0.034).abs() < 1e-9);
        assert!((u.total_minutes() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn month_change_zeroes_month_not_total() {
        let mut u = UsageStore::default();
        u.add_samples(24_000 * 120, "2026-05"); // 2 min
        u.add_samples(24_000 * 60, "2026-06"); // new month, 1 min
        assert!((u.month_minutes() - 1.0).abs() < 1e-9);
        assert!((u.total_minutes() - 3.0).abs() < 1e-9);
        assert_eq!(u.month, "2026-06");
    }
}

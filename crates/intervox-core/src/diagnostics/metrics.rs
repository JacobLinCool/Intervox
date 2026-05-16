//! Latency metrics (spec §12). The UI shows only a coarse "Latency: 1.2s";
//! advanced logs can show every field.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyMetrics {
    pub capture_to_send_ms: u32,
    pub openai_first_audio_ms: u32,
    pub jitter_buffer_ms: u32,
    pub virtual_mic_output_lag_ms: u32,
    pub total_estimated_latency_ms: u32,
}

impl LatencyMetrics {
    /// Recompute `total_estimated_latency_ms` from the components.
    pub fn recompute_total(&mut self) -> u32 {
        self.total_estimated_latency_ms = self
            .capture_to_send_ms
            .saturating_add(self.openai_first_audio_ms)
            .saturating_add(self.jitter_buffer_ms)
            .saturating_add(self.virtual_mic_output_lag_ms);
        self.total_estimated_latency_ms
    }

    /// Coarse display string for the main UI, e.g. 1234 ms -> "1.2s".
    pub fn display_seconds(&self) -> String {
        format!("{:.1}s", self.total_estimated_latency_ms as f32 / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_sums_components() {
        let mut m = LatencyMetrics {
            capture_to_send_ms: 30,
            openai_first_audio_ms: 800,
            jitter_buffer_ms: 120,
            virtual_mic_output_lag_ms: 20,
            total_estimated_latency_ms: 0,
        };
        assert_eq!(m.recompute_total(), 970);
        assert_eq!(m.total_estimated_latency_ms, 970);
    }

    #[test]
    fn display_seconds_one_decimal() {
        let m = LatencyMetrics {
            total_estimated_latency_ms: 1234,
            ..Default::default()
        };
        assert_eq!(m.display_seconds(), "1.2s");
    }

    #[test]
    fn serde_camel_case_shape() {
        let j = serde_json::to_string(&LatencyMetrics::default()).unwrap();
        assert!(j.contains("captureToSendMs"));
        assert!(j.contains("totalEstimatedLatencyMs"));
    }
}

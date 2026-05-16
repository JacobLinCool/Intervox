//! Caption transcript accumulation (spec §4.2, §13). Source and target are
//! independent streams of deltas; segments finalise on `is_final`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptDelta {
    pub text: String,
    pub is_final: bool,
}

#[derive(Debug, Default, Clone)]
struct Stream {
    finalized: String,
    pending: String,
}

impl Stream {
    fn apply(&mut self, d: &TranscriptDelta) {
        self.pending.push_str(&d.text);
        if d.is_final {
            if !self.finalized.is_empty() {
                self.finalized.push(' ');
            }
            self.finalized.push_str(self.pending.trim());
            self.pending.clear();
        }
    }

    fn full(&self) -> String {
        if self.pending.is_empty() {
            self.finalized.clone()
        } else if self.finalized.is_empty() {
            self.pending.clone()
        } else {
            format!("{} {}", self.finalized, self.pending)
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct TranscriptState {
    source: Stream,
    target: Stream,
}

impl TranscriptState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_source(&mut self, d: &TranscriptDelta) {
        self.source.apply(d);
    }

    pub fn apply_target(&mut self, d: &TranscriptDelta) {
        self.target.apply(d);
    }

    pub fn source_text(&self) -> String {
        self.source.full()
    }

    pub fn target_text(&self) -> String {
        self.target.full()
    }

    pub fn clear(&mut self) {
        self.source = Stream::default();
        self.target = Stream::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(t: &str, f: bool) -> TranscriptDelta {
        TranscriptDelta {
            text: t.into(),
            is_final: f,
        }
    }

    #[test]
    fn accumulates_per_stream() {
        let mut s = TranscriptState::new();
        s.apply_source(&d("你", false));
        s.apply_source(&d("好", false));
        s.apply_target(&d("hel", false));
        s.apply_target(&d("lo", false));
        assert_eq!(s.source_text(), "你好");
        assert_eq!(s.target_text(), "hello");
    }

    #[test]
    fn final_delta_finalizes_segment() {
        let mut s = TranscriptState::new();
        s.apply_target(&d("hello", true));
        s.apply_target(&d("world", true));
        assert_eq!(s.target_text(), "hello world");
    }

    #[test]
    fn clear_resets_both_streams() {
        let mut s = TranscriptState::new();
        s.apply_source(&d("x", true));
        s.apply_target(&d("y", true));
        s.clear();
        assert_eq!(s.source_text(), "");
        assert_eq!(s.target_text(), "");
    }

    #[test]
    fn serde_shape_camel_case() {
        let j = serde_json::to_string(&d("hi", true)).unwrap();
        assert_eq!(j, r#"{"text":"hi","isFinal":true}"#);
    }
}

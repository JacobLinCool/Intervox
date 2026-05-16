//! OpenAI Realtime Translation protocol model (spec §8). Pure: turns server
//! JSON into typed events and builds the `session.update` payload. The actual
//! websocket transport is a thin adapter layered on this later.

use crate::audio::pcm;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Closing,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TranslationEvent {
    SessionUpdated,
    OutputAudioDelta {
        pcm16: Vec<i16>,
        sample_rate: u32,
        channels: u16,
        elapsed_ms: Option<u64>,
    },
    OutputTranscriptDelta {
        text: String,
        elapsed_ms: Option<u64>,
    },
    InputTranscriptDelta {
        text: String,
        elapsed_ms: Option<u64>,
    },
    Error {
        code: Option<String>,
        message: String,
    },
    Closed,
    /// Unrecognised event types are surfaced, not panicked on.
    Ignored(String),
}

/// Build the `session.update` payload (spec §8.3) for source→target.
pub fn build_session_update(source_language: &str, target_language: &str) -> Value {
    json!({
        "type": "session.update",
        "session": {
            "audio": {
                "input": {
                    "noise_reduction": { "type": "near_field" },
                    "transcription": {
                        "model": "gpt-realtime-whisper",
                        "language": source_language
                    }
                },
                "output": { "language": target_language }
            }
        }
    })
}

fn elapsed(v: &Value) -> Option<u64> {
    v.get("elapsed_ms")
        .and_then(|e| e.as_u64())
        .or_else(|| v.get("audio_end_ms").and_then(|e| e.as_u64()))
}

/// Parse one server JSON message into a `TranslationEvent`. Unknown or
/// malformed input yields `Ignored`/`Error` rather than panicking.
pub fn parse_server_event(raw: &str) -> TranslationEvent {
    let v: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            return TranslationEvent::Error {
                code: Some("PARSE".into()),
                message: e.to_string(),
            }
        }
    };
    let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match ty {
        "session.updated" | "session.created" => TranslationEvent::SessionUpdated,

        t if t.contains("input_audio_transcription") && t.ends_with("delta") => {
            // source transcript (spec §8.4 InputTranscriptDelta) — check this
            // before the generic audio+transcript arm, which would otherwise
            // also match (it contains "audio", "transcript", ends "delta").
            TranslationEvent::InputTranscriptDelta {
                text: v
                    .get("delta")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
                elapsed_ms: elapsed(&v),
            }
        }

        t if t.contains("audio") && t.contains("transcript") && t.ends_with("delta") => {
            // e.g. response.output_audio_transcript.delta (translated text)
            TranslationEvent::OutputTranscriptDelta {
                text: v
                    .get("delta")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
                elapsed_ms: elapsed(&v),
            }
        }

        t if t.contains("audio") && t.ends_with("delta") => {
            // translated audio: base64 PCM16 in `delta`
            let b64 = v.get("delta").and_then(|d| d.as_str()).unwrap_or("");
            match pcm::base64_to_pcm16(b64) {
                Ok(pcm16) => TranslationEvent::OutputAudioDelta {
                    pcm16,
                    sample_rate: v
                        .get("sample_rate")
                        .and_then(|s| s.as_u64())
                        .unwrap_or(24000) as u32,
                    channels: v.get("channels").and_then(|c| c.as_u64()).unwrap_or(1) as u16,
                    elapsed_ms: elapsed(&v),
                },
                Err(e) => TranslationEvent::Error {
                    code: Some("AUDIO_DECODE".into()),
                    message: e,
                },
            }
        }

        "error" => {
            let err = v.get("error").unwrap_or(&v);
            TranslationEvent::Error {
                code: err
                    .get("code")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string()),
                message: err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error")
                    .to_string(),
            }
        }

        "" => TranslationEvent::Ignored("missing type".into()),
        other => TranslationEvent::Ignored(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_update_matches_spec_8_3() {
        let v = build_session_update("zh", "en");
        assert_eq!(v["type"], "session.update");
        assert_eq!(
            v["session"]["audio"]["input"]["noise_reduction"]["type"],
            "near_field"
        );
        assert_eq!(
            v["session"]["audio"]["input"]["transcription"]["model"],
            "gpt-realtime-whisper"
        );
        assert_eq!(v["session"]["audio"]["input"]["transcription"]["language"], "zh");
        assert_eq!(v["session"]["audio"]["output"]["language"], "en");
    }

    #[test]
    fn parses_session_updated() {
        assert_eq!(
            parse_server_event(r#"{"type":"session.updated"}"#),
            TranslationEvent::SessionUpdated
        );
    }

    #[test]
    fn parses_translated_audio_delta() {
        let b64 = pcm::pcm16_to_base64(&[100, -100, 32767]);
        let raw = format!(
            r#"{{"type":"response.output_audio.delta","delta":"{b64}","elapsed_ms":640}}"#
        );
        match parse_server_event(&raw) {
            TranslationEvent::OutputAudioDelta {
                pcm16,
                sample_rate,
                elapsed_ms,
                ..
            } => {
                assert_eq!(pcm16, vec![100, -100, 32767]);
                assert_eq!(sample_rate, 24000);
                assert_eq!(elapsed_ms, Some(640));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_target_transcript_delta() {
        let raw = r#"{"type":"response.output_audio_transcript.delta","delta":"hello"}"#;
        assert_eq!(
            parse_server_event(raw),
            TranslationEvent::OutputTranscriptDelta {
                text: "hello".into(),
                elapsed_ms: None
            }
        );
    }

    #[test]
    fn parses_input_transcript_delta() {
        let raw =
            r#"{"type":"conversation.item.input_audio_transcription.delta","delta":"你好"}"#;
        assert_eq!(
            parse_server_event(raw),
            TranslationEvent::InputTranscriptDelta {
                text: "你好".into(),
                elapsed_ms: None
            }
        );
    }

    #[test]
    fn parses_error() {
        let raw = r#"{"type":"error","error":{"code":"rate_limit","message":"slow down"}}"#;
        assert_eq!(
            parse_server_event(raw),
            TranslationEvent::Error {
                code: Some("rate_limit".into()),
                message: "slow down".into()
            }
        );
    }

    #[test]
    fn unknown_type_is_ignored_not_panic() {
        assert_eq!(
            parse_server_event(r#"{"type":"response.created"}"#),
            TranslationEvent::Ignored("response.created".into())
        );
    }

    #[test]
    fn malformed_json_is_error_not_panic() {
        match parse_server_event("not json") {
            TranslationEvent::Error { .. } => {}
            other => panic!("got {other:?}"),
        }
    }
}

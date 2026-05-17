//! OpenAI Realtime Translation protocol — official `/v1/realtime/translations`
//! endpoint (model `gpt-realtime-translate`). Pure: turns server JSON into
//! typed events and builds the `session.update` payload. The actual websocket
//! transport is the native websocket adapter in the Tauri runtime.
//!
//! References:
//! - <https://developers.openai.com/api/docs/guides/realtime-translation>
//! - GA migration notes (2025): `OpenAI-Beta: realtime=v1` header removed.

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
    InputTranscriptDone,
    OutputTranscriptDone,
    Error {
        code: Option<String>,
        message: String,
    },
    Closed,
    /// Unrecognised event types are surfaced, not panicked on.
    Ignored(String),
}

/// Build the `session.update` payload for the official `/v1/realtime/translations`
/// endpoint.
///
/// Per the official translations guide the only required session field is the
/// **target output language** (`session.audio.output.language`).  The source
/// language is auto-detected by the endpoint and is not configurable, so it is
/// not part of this payload.
///
/// Do NOT include `session.audio.input` (fields like `noise_reduction` or
/// `transcription.model`) — they trigger GA type errors on the translations
/// endpoint and reference the non-existent model `gpt-realtime-whisper`.
pub fn build_session_update(target_language: &str) -> Value {
    json!({
        "type": "session.update",
        "session": {
            "audio": {
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
///
/// Event names are the official GA `session.*` wire format from the
/// `/v1/realtime/translations` endpoint.
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

    // Helper closures that extract the delta text / audio from the parsed value.
    let delta_str = || {
        v.get("delta")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string()
    };

    match ty {
        // ── Session lifecycle (official GA names) ─────────────────────────────
        "session.created" | "session.updated" => TranslationEvent::SessionUpdated,

        // ── Translated audio ─────────────────────────────────────────────────
        // SPEC: official field is `session.output_audio.delta`; `delta` carries
        // base64 PCM16 at 24 kHz mono.
        "session.output_audio.delta" => {
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

        // ── Source-language transcript ───────────────────────────────────────
        // SPEC: official field is `session.input_transcript.delta`.
        "session.input_transcript.delta" => TranslationEvent::InputTranscriptDelta {
            text: delta_str(),
            elapsed_ms: elapsed(&v),
        },

        // ── Translated transcript ────────────────────────────────────────────
        // SPEC: official field is `session.output_transcript.delta`.
        "session.output_transcript.delta" => TranslationEvent::OutputTranscriptDelta {
            text: delta_str(),
            elapsed_ms: elapsed(&v),
        },

        // ── Segment finalization ─────────────────────────────────────────────
        "session.input_transcript.done" => TranslationEvent::InputTranscriptDone,
        "session.output_transcript.done" => TranslationEvent::OutputTranscriptDone,

        // ── Error ─────────────────────────────────────────────────────────────
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

    // ── build_session_update ──────────────────────────────────────────────────

    /// Asserts the OFFICIAL minimal shape: only `session.audio.output.language`.
    /// No `session.audio.input` block (that would trigger GA type errors).
    #[test]
    fn session_update_matches_spec_8_3() {
        let v = build_session_update("en");
        assert_eq!(v["type"], "session.update");
        // target language is present
        assert_eq!(v["session"]["audio"]["output"]["language"], "en");
        // input block must NOT be present (no gpt-realtime-whisper, no noise_reduction)
        assert!(
            v["session"]["audio"]["input"].is_null(),
            "session.audio.input must not be present in the official shape; got: {}",
            v["session"]["audio"]["input"]
        );
    }

    #[test]
    fn session_update_target_language_is_forwarded() {
        let v = build_session_update("fr");
        assert_eq!(v["session"]["audio"]["output"]["language"], "fr");
    }

    // ── parse_server_event — official GA session.* names (primary paths) ─────

    /// Official: `session.created` → SessionUpdated (connected).
    #[test]
    fn parses_session_created() {
        assert_eq!(
            parse_server_event(r#"{"type":"session.created"}"#),
            TranslationEvent::SessionUpdated
        );
    }

    /// Official: `session.updated` → SessionUpdated.
    #[test]
    fn parses_session_updated() {
        assert_eq!(
            parse_server_event(r#"{"type":"session.updated"}"#),
            TranslationEvent::SessionUpdated
        );
    }

    /// Official: `session.output_audio.delta` — base64 PCM16 in `delta`.
    #[test]
    fn parses_session_output_audio_delta() {
        let b64 = pcm::pcm16_to_base64(&[100, -100, 32767]);
        let raw =
            format!(r#"{{"type":"session.output_audio.delta","delta":"{b64}","elapsed_ms":640}}"#);
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

    /// Official: `session.input_transcript.delta` — source-language text.
    #[test]
    fn parses_session_input_transcript_delta() {
        let raw = r#"{"type":"session.input_transcript.delta","delta":"你好"}"#;
        assert_eq!(
            parse_server_event(raw),
            TranslationEvent::InputTranscriptDelta {
                text: "你好".into(),
                elapsed_ms: None
            }
        );
    }

    /// Official: `session.output_transcript.delta` — translated text.
    #[test]
    fn parses_session_output_transcript_delta() {
        let raw = r#"{"type":"session.output_transcript.delta","delta":"hello"}"#;
        assert_eq!(
            parse_server_event(raw),
            TranslationEvent::OutputTranscriptDelta {
                text: "hello".into(),
                elapsed_ms: None
            }
        );
    }

    // ── segment finalization ─────────────────────────────────────────────────

    #[test]
    fn parses_input_transcript_done() {
        assert_eq!(
            parse_server_event(r#"{"type":"session.input_transcript.done"}"#),
            TranslationEvent::InputTranscriptDone
        );
    }

    #[test]
    fn parses_output_transcript_done() {
        assert_eq!(
            parse_server_event(r#"{"type":"session.output_transcript.done"}"#),
            TranslationEvent::OutputTranscriptDone
        );
    }

    // ── error / edge cases ────────────────────────────────────────────────────

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
    fn missing_type_is_ignored() {
        assert_eq!(
            parse_server_event(r#"{}"#),
            TranslationEvent::Ignored("missing type".into())
        );
    }

    #[test]
    fn malformed_json_is_error_not_panic() {
        match parse_server_event("not json") {
            TranslationEvent::Error { .. } => {}
            other => panic!("got {other:?}"),
        }
    }

    // ── base64 round-trip for PCM16 ───────────────────────────────────────────

    #[test]
    fn pcm16_base64_round_trip_via_session_output_audio_delta() {
        let samples: Vec<i16> = vec![0, 1000, -1000, i16::MAX, i16::MIN];
        let b64 = pcm::pcm16_to_base64(&samples);
        let raw = format!(r#"{{"type":"session.output_audio.delta","delta":"{b64}"}}"#);
        match parse_server_event(&raw) {
            TranslationEvent::OutputAudioDelta { pcm16, .. } => {
                assert_eq!(pcm16, samples);
            }
            other => panic!("got {other:?}"),
        }
    }
}

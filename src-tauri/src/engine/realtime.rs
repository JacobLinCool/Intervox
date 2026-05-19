//! OpenAI Realtime Translation websocket transport.
//!
//! # Protocol
//!
//! Speaks the official GA `/v1/realtime/translations` wire protocol. The
//! endpoint URL is supplied by the caller: it defaults to [`REALTIME_URL`]
//! (OpenAI) but can be any wire-compatible server, e.g. a self-hosted
//! `open-realtime-translate` instance at `ws://127.0.0.1:8000/...`.
//! - Headers: `Authorization: Bearer <key>` and `OpenAI-Safety-Identifier:
//!   <install-id>` — sent only when a non-empty key is provided. An empty key
//!   (used for self-hosted endpoints that do not validate credentials) sends
//!   neither header. The `OpenAI-Beta: realtime=v1` header was removed in GA
//!   and must NOT be sent.
//! - Uplink audio event type: `session.input_audio_buffer.append`.
//! - Server-side VAD: no `input_audio_buffer.commit` sent — the server auto-commits turns.
//!
//! # Architecture
//!
//! `run` is a long-lived async task managed by the `Engine`.  It:
//!   1. Connects to the OpenAI Realtime WebSocket endpoint with the user's API key.
//!   2. Sends a `session.update` to configure the target output language.
//!   3. Forwards captured 24 kHz PCM16 frames from the engine graph loop as
//!      `session.input_audio_buffer.append` messages (uplink / mic → server).
//!   4. Receives server events and forwards them to `ev_tx` for Task 4.2 to consume.
//!   5. Reconnects with capped exponential backoff on any transport error.
//!
//! # Lifecycle
//!
//! The task is cancelled via `JoinHandle::abort()` from the Engine.  All inner
//! loops yield at `.await` points so cancellation is prompt.
//!
//! # Privacy
//!
//! This module NEVER logs the API key, audio bytes, or transcript text.
//! The `OpenAI-Safety-Identifier` is a stable, anonymous, non-PII install ID.

use futures_util::{SinkExt, StreamExt};
use intervox_core::realtime::events::{build_session_update, parse_server_event, TranslationEvent};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest, handshake::client::Request, http::HeaderName,
        http::HeaderValue, Error as WsError, Message,
    },
};

/// Default OpenAI Realtime Translation WebSocket endpoint (GA, 2025).
///
/// Used when no custom endpoint is configured. See
/// <https://developers.openai.com/api/docs/guides/realtime-translation>.
pub const REALTIME_URL: &str =
    "wss://api.openai.com/v1/realtime/translations?model=gpt-realtime-translate";

/// Failure modes when assembling the WebSocket upgrade request. Variants never
/// carry the API key.
#[derive(Debug)]
enum RequestError {
    /// The endpoint URL could not be parsed into an HTTP upgrade request.
    Url,
    /// The API key contains characters invalid for an HTTP header value.
    KeyHeader,
    /// The anonymous OpenAI safety identifier could not be created.
    SafetyId(String),
    /// The safety identifier is not a valid HTTP header value.
    SafetyIdHeader,
}

/// Build the WebSocket upgrade request for `url`.
///
/// - `url`: full `ws://` / `wss://` endpoint, including the `?model=` query.
/// - `key`: Bearer API key. An **empty** key (after trim) means *no auth* —
///   neither `Authorization` nor `OpenAI-Safety-Identifier` is sent, which is
///   the path used for wire-compatible self-hosted endpoints that do not
///   validate credentials. A non-empty key reproduces the exact OpenAI GA
///   header set.
///
/// The key is never logged, including via the returned error.
fn build_request(url: &str, key: &str) -> Result<Request, RequestError> {
    let mut request = url.into_client_request().map_err(|_| RequestError::Url)?;

    let key = key.trim();
    if !key.is_empty() {
        let headers = request.headers_mut();

        // Authorization header — key is NOT logged anywhere.
        let auth_value = HeaderValue::from_str(&format!("Bearer {key}"))
            .map_err(|_| RequestError::KeyHeader)?;
        headers.insert(
            tokio_tungstenite::tungstenite::http::header::AUTHORIZATION,
            auth_value,
        );

        // NOTE: `OpenAI-Beta: realtime=v1` was removed in the GA release of
        // /v1/realtime/translations.  Do NOT re-add it.

        // Stable, anonymous, non-PII install identifier required by the GA
        // translations endpoint. Value must not be logged.
        let sid = safety_identifier().map_err(RequestError::SafetyId)?;
        let sid_val = HeaderValue::from_str(&sid).map_err(|_| RequestError::SafetyIdHeader)?;
        headers.insert(HeaderName::from_static("openai-safety-identifier"), sid_val);
    }

    Ok(request)
}

/// Capped exponential backoff for reconnect attempts.
///
/// - `attempt == 0`: 250 ms (first reconnect is nearly immediate).
/// - Doubles each attempt.
/// - Hard cap at 5 000 ms (5 s) to avoid runaway back-off.
///
/// This is a pure function — tested in the unit-test section below.
pub fn backoff(attempt: u32) -> std::time::Duration {
    const BASE_MS: u64 = 250;
    const CAP_MS: u64 = 5_000;
    // Saturating left-shift via checked_shl + checked_mul to avoid overflow
    // on large `attempt` values.  u64::MAX >> 1 is the safe upper bound for
    // the shift count; beyond that we just return the cap immediately.
    let ms = if attempt >= 64 {
        // 250 * 2^64 overflows u64 — return cap directly.
        CAP_MS
    } else {
        // 2_u64.checked_shl(attempt) is always Some for attempt < 64.
        let multiplier = 1_u64.checked_shl(attempt).unwrap_or(u64::MAX);
        BASE_MS.saturating_mul(multiplier).min(CAP_MS)
    };
    std::time::Duration::from_millis(ms)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExit {
    Terminal,
}

fn handshake_status(err: &WsError) -> Option<u16> {
    match err {
        WsError::Http(response) => Some(response.status().as_u16()),
        _ => None,
    }
}

/// Return a stable, anonymous, non-PII install identifier for the
/// `OpenAI-Safety-Identifier` header.
///
/// The value is a 32-character lowercase hex string derived from 16 bytes read
/// from `/dev/urandom` the first time it is needed; it is persisted to
/// `~/Library/Application Support/app.intervox.desktop/install-id` (or the
/// platform equivalent) so that the same identifier is reused across launches.
///
/// Creation or persistence failure is terminal for a realtime session. A fixed
/// shared identifier would hide install-specific errors and break the stability
/// contract.
///
/// The value is not PII and must never be logged.
/// In-process cache so `safety_identifier()` returns the same value within a
/// single process run regardless of how many times it is called.
static SAFETY_ID: std::sync::OnceLock<Result<String, String>> = std::sync::OnceLock::new();

pub fn safety_identifier() -> Result<String, String> {
    SAFETY_ID.get_or_init(resolve_safety_identifier).clone()
}

fn resolve_safety_identifier() -> Result<String, String> {
    use std::io::Read;

    let base =
        dirs::config_dir().ok_or_else(|| "platform config directory is unavailable".to_string())?;
    let dir = base.join("app.intervox.desktop");
    let id_path = dir.join("install-id");

    if let Ok(existing) = std::fs::read_to_string(&id_path) {
        let trimmed = existing.trim().to_string();
        if trimmed.len() == 32
            && trimmed
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        {
            return Ok(trimmed);
        }
    }

    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create install-id directory: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("cannot protect install-id directory: {e}"))?;
    }

    let mut f = std::fs::File::open("/dev/urandom")
        .map_err(|e| format!("cannot open /dev/urandom: {e}"))?;
    let mut buf = [0u8; 16];
    f.read_exact(&mut buf)
        .map_err(|e| format!("cannot read random install-id bytes: {e}"))?;
    let new_id: String = buf.iter().map(|b| format!("{b:02x}")).collect();

    std::fs::write(&id_path, &new_id).map_err(|e| format!("cannot persist install-id: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&id_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("cannot protect install-id: {e}"))?;
    }

    Ok(new_id)
}

/// Run the Realtime Translation transport.
///
/// # Parameters
/// - `url`: The full `ws://` / `wss://` endpoint (default [`REALTIME_URL`] or a
///   custom wire-compatible server).
/// - `key`: The Bearer API key (never logged). An empty string means *no auth*:
///   neither `Authorization` nor `OpenAI-Safety-Identifier` is sent (used for
///   self-hosted endpoints that do not validate credentials).
/// - `tgt_lang`: BCP-47 target output language code (source is auto-detected
///   by the endpoint and is not configurable).
/// - `pcm_rx`: Incoming 24 kHz mono PCM16 frames from the graph loop (uplink).
/// - `ev_tx`: Outgoing server events for the downstream consumer (Task 4.2 stub).
///
/// The future runs until:
/// - `pcm_rx` is closed (capture stopped / engine shutting down), or
/// - `ev_tx` is closed (consumer gone / engine shutting down), or
/// - The task is aborted via `JoinHandle::abort()`.
pub async fn run(
    url: String,
    key: String,
    tgt_lang: String,
    mut pcm_rx: tokio::sync::mpsc::Receiver<Vec<i16>>,
    ev_tx: tokio::sync::mpsc::Sender<TranslationEvent>,
    uplink_samples: std::sync::Arc<std::sync::atomic::AtomicU64>,
) -> RunExit {
    let mut attempt: u32 = 0;

    loop {
        // ── Build the HTTP upgrade request ────────────────────────────────────
        let request = match build_request(&url, &key) {
            Ok(r) => r,
            Err(RequestError::Url) => {
                // Malformed endpoint URL — non-retriable; never log the URL or
                // key. Surface to the UI so a bad custom endpoint is visible.
                eprintln!("[realtime] endpoint URL parse error");
                let _ = ev_tx
                    .send(TranslationEvent::Error {
                        code: Some("ENDPOINT".into()),
                        message: "Realtime endpoint URL is invalid".into(),
                    })
                    .await;
                return RunExit::Terminal;
            }
            Err(RequestError::KeyHeader) => {
                eprintln!("[realtime] invalid key format for header");
                // Key is bad; no point retrying — surface error and exit.
                let _ = ev_tx
                    .send(TranslationEvent::Error {
                        code: Some("AUTH".into()),
                        message: "API key contains invalid header characters".into(),
                    })
                    .await;
                return RunExit::Terminal;
            }
            Err(RequestError::SafetyId(e)) => {
                eprintln!("[realtime] safety identifier error: {e}");
                let _ = ev_tx
                    .send(TranslationEvent::Error {
                        code: Some("SAFETY_IDENTIFIER".into()),
                        message: "Cannot create the anonymous OpenAI safety identifier".into(),
                    })
                    .await;
                return RunExit::Terminal;
            }
            Err(RequestError::SafetyIdHeader) => {
                eprintln!("[realtime] invalid safety identifier header");
                let _ = ev_tx
                    .send(TranslationEvent::Error {
                        code: Some("SAFETY_IDENTIFIER".into()),
                        message: "Anonymous OpenAI safety identifier is invalid".into(),
                    })
                    .await;
                return RunExit::Terminal;
            }
        };

        // ── Connect ───────────────────────────────────────────────────────────
        match connect_async(request).await {
            Ok((ws_stream, _response)) => {
                // Successful connection — reset backoff.
                attempt = 0;

                let (mut sink, mut stream) = ws_stream.split();

                // Send session.update immediately on open.
                let session_msg = build_session_update(&tgt_lang).to_string();
                if let Err(e) = sink.send(Message::Text(session_msg)).await {
                    eprintln!("[realtime] failed to send session.update: {e}");
                    // Fall through to reconnect.
                } else {
                    // ── Main select loop: uplink (send) + downlink (recv) ─────
                    let reconnect = loop {
                        tokio::select! {
                            // ── Uplink: mic → server ──────────────────────────
                            frame = pcm_rx.recv() => {
                                match frame {
                                    None => {
                                        // Channel closed — engine stopped capture.
                                        // Exit cleanly without reconnecting.
                                        let _ = ev_tx.send(TranslationEvent::Closed).await;
                                        return RunExit::Terminal;
                                    }
                                    Some(pcm) => {
                                        let b64 = intervox_core::audio::pcm::pcm16_to_base64(&pcm);
                                        let msg = serde_json::json!({
                                            // Official GA uplink event name (session. prefix).
                                            "type": "session.input_audio_buffer.append",
                                            "audio": b64
                                            // NOTE: No commit sent — server-side VAD auto-commits
                                            // turns on the /v1/realtime/translations endpoint.
                                        })
                                        .to_string();
                                        if let Err(_e) = sink.send(Message::Text(msg)).await {
                                            // Do not interpolate the error — tungstenite Error
                                            // variants can embed message payloads (base64 audio).
                                            eprintln!("[realtime] send error");
                                            break true; // reconnect
                                        }
                                        uplink_samples.fetch_add(
                                            pcm.len() as u64,
                                            std::sync::atomic::Ordering::Relaxed,
                                        );
                                    }
                                }
                            }

                            // ── Downlink: server → ev_tx ──────────────────────
                            ws_msg = stream.next() => {
                                match ws_msg {
                                    None => {
                                        // Stream ended — reconnect.
                                        break true;
                                    }
                                    Some(Err(e)) => {
                                        eprintln!("[realtime] recv error: {e}");
                                        break true; // reconnect
                                    }
                                    Some(Ok(Message::Text(s))) => {
                                        let ev = parse_server_event(&s);
                                        if ev_tx.send(ev).await.is_err() {
                                            // Consumer gone — exit cleanly.
                                            return RunExit::Terminal;
                                        }
                                    }
                                    Some(Ok(Message::Binary(bytes))) => {
                                        // Binary frames: try to interpret as UTF-8 JSON
                                        // (the core parser expects text JSON with base64 in `delta`).
                                        if let Ok(s) = std::str::from_utf8(&bytes) {
                                            let ev = parse_server_event(s);
                                            if ev_tx.send(ev).await.is_err() {
                                                return RunExit::Terminal;
                                            }
                                        }
                                        // Non-UTF-8 binary: ignore silently.
                                    }
                                    Some(Ok(Message::Ping(data))) => {
                                        // Reply with Pong to satisfy the server's keep-alive.
                                        let _ = sink.send(Message::Pong(data)).await;
                                    }
                                    Some(Ok(Message::Pong(_))) => {
                                        // Unsolicited pong — ignore.
                                    }
                                    Some(Ok(Message::Close(_))) => {
                                        // Server closed the connection — reconnect.
                                        let _ = ev_tx.send(TranslationEvent::Closed).await;
                                        break true;
                                    }
                                    Some(Ok(Message::Frame(_))) => {
                                        // Raw frame — ignore (should not occur at this level).
                                    }
                                }
                            }
                        }
                    };

                    if !reconnect {
                        return RunExit::Terminal;
                    }
                }

                // Best-effort: signal closed before backoff.
                let _ = ev_tx.send(TranslationEvent::Closed).await;
            }
            Err(e) => {
                if matches!(handshake_status(&e), Some(401 | 403)) {
                    let _ = ev_tx
                        .send(TranslationEvent::Error {
                            code: Some("AUTH".into()),
                            message:
                                "OpenAI rejected the API key for the realtime translation session"
                                    .into(),
                        })
                        .await;
                    return RunExit::Terminal;
                }
                // Do not interpolate the error — tungstenite connect errors can
                // embed the full HTTP response (including echoed request headers
                // such as Authorization).  Log only the attempt counter.
                eprintln!("[realtime] connect error (attempt {attempt})");
            }
        }

        // ── Backoff before reconnect ──────────────────────────────────────────
        let delay = backoff(attempt);
        attempt = attempt.saturating_add(1);

        // Check if the channel is already closed before sleeping.
        if ev_tx.is_closed() {
            return RunExit::Terminal;
        }

        tokio::time::sleep(delay).await;

        // After waking, check again — pcm_rx may have been dropped.
        if ev_tx.is_closed() {
            return RunExit::Terminal;
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{build_request, backoff, safety_identifier, RequestError, REALTIME_URL};
    use std::time::Duration;

    // ── build_request ─────────────────────────────────────────────────────────

    const AUTH: &str = "authorization";
    const SID: &str = "openai-safety-identifier";

    #[test]
    fn build_request_empty_key_sends_no_auth_headers() {
        // Self-hosted / wire-compatible endpoint that does not validate creds.
        let url = "ws://127.0.0.1:8000/v1/realtime/translations?model=gpt-realtime-translate";
        let req = build_request(url, "").expect("plain ws url must build");
        assert!(
            !req.headers().contains_key(AUTH),
            "empty key must NOT send Authorization"
        );
        assert!(
            !req.headers().contains_key(SID),
            "empty key must NOT send the safety identifier"
        );
        // Whitespace-only key is also treated as "no auth".
        let req2 = build_request(url, "   ").expect("ws url must build");
        assert!(!req2.headers().contains_key(AUTH));
        assert!(!req2.headers().contains_key(SID));
    }

    #[test]
    fn build_request_with_key_sends_openai_headers() {
        let req = build_request(REALTIME_URL, "sk-test-key-1234567890")
            .expect("default url + key must build");
        let auth = req
            .headers()
            .get(AUTH)
            .expect("Authorization must be present")
            .to_str()
            .unwrap();
        assert_eq!(auth, "Bearer sk-test-key-1234567890");
        assert!(
            req.headers().contains_key(SID),
            "a non-empty key must also send the safety identifier"
        );
    }

    #[test]
    fn build_request_rejects_malformed_url() {
        // A space in the authority is not a valid URI and cannot be parsed
        // into an HTTP upgrade request.
        match build_request("ws://bad host/path", "") {
            Err(RequestError::Url) => {}
            other => panic!("expected Err(RequestError::Url), got {other:?}"),
        }
    }

    /// Red → Green: backoff must implement 250 ms base, doubling, 5 s cap.
    ///
    /// Verified values:
    ///   attempt 0 → 250 ms  (250 * 2^0 = 250)
    ///   attempt 1 → 500 ms  (250 * 2^1 = 500)
    ///   attempt 2 → 1 000 ms (250 * 2^2 = 1000)
    ///   attempt 3 → 2 000 ms (250 * 2^3 = 2000)
    ///   attempt 4 → 4 000 ms (250 * 2^4 = 4000)
    ///   attempt 5 → 5 000 ms (250 * 2^5 = 8000, capped at 5000)
    ///   attempt 100 → 5 000 ms (far above cap)
    #[test]
    fn backoff_durations_are_correct() {
        assert_eq!(backoff(0), Duration::from_millis(250), "attempt 0");
        assert_eq!(backoff(1), Duration::from_millis(500), "attempt 1");
        assert_eq!(backoff(2), Duration::from_millis(1_000), "attempt 2");
        assert_eq!(backoff(3), Duration::from_millis(2_000), "attempt 3");
        assert_eq!(backoff(4), Duration::from_millis(4_000), "attempt 4");
        assert_eq!(backoff(5), Duration::from_millis(5_000), "attempt 5 (cap)");
        assert_eq!(
            backoff(100),
            Duration::from_millis(5_000),
            "attempt 100 (cap)"
        );
    }

    #[test]
    fn backoff_never_exceeds_cap() {
        for attempt in 0_u32..=200 {
            assert!(
                backoff(attempt) <= Duration::from_millis(5_000),
                "attempt {attempt} exceeded 5 s cap: {:?}",
                backoff(attempt)
            );
        }
    }

    #[test]
    fn backoff_is_monotonically_non_decreasing_until_cap() {
        let durations: Vec<Duration> = (0_u32..=10).map(backoff).collect();
        for w in durations.windows(2) {
            assert!(w[0] <= w[1], "not non-decreasing: {:?} > {:?}", w[0], w[1]);
        }
    }

    // ── Protocol constants ────────────────────────────────────────────────────

    #[test]
    fn realtime_url_points_to_official_translations_endpoint() {
        assert!(
            REALTIME_URL.contains("/v1/realtime/translations"),
            "URL must target /v1/realtime/translations"
        );
        assert!(
            REALTIME_URL.contains("gpt-realtime-translate"),
            "URL must use model=gpt-realtime-translate"
        );
    }

    // ── safety_identifier ─────────────────────────────────────────────────────

    #[test]
    fn safety_identifier_is_non_empty_and_ascii() {
        let id = safety_identifier().expect("safety identifier should resolve on macOS");
        assert!(!id.is_empty(), "safety_identifier must not be empty");
        assert!(
            id.is_ascii(),
            "safety_identifier must be ASCII (got: {id:?})"
        );
        assert_eq!(id.len(), 32, "safety_identifier must be 16 bytes as hex");
        assert!(
            id.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "safety_identifier must be lowercase hex"
        );
    }

    #[test]
    fn safety_identifier_is_stable_across_calls() {
        // Call twice: the identifier must stay stable for a single app install.
        let id1 = safety_identifier().expect("safety identifier should resolve on macOS");
        let id2 = safety_identifier().expect("safety identifier should resolve on macOS");
        assert_eq!(id1, id2, "safety_identifier must be stable across calls");
    }

    #[test]
    fn safety_identifier_contains_no_pii_markers() {
        let id = safety_identifier().expect("safety identifier should resolve on macOS");
        // Must not contain the literal word "user", "email", IP patterns, or sk-
        assert!(
            !id.contains("sk-"),
            "safety_identifier must not contain API key prefix"
        );
        assert!(
            !id.contains('@'),
            "safety_identifier must not contain email address"
        );
    }
}

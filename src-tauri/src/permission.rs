//! Microphone permission status (macOS AVFoundation) and privacy pane helpers.

use std::sync::mpsc;

use serde::{Deserialize, Serialize};

/// Mirrors `AVAuthorizationStatus` for the microphone media type.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MicPermission {
    Granted,
    Denied,
    NotDetermined,
    Restricted,
}

/// Returns the current microphone authorization status from the OS.
///
/// Queries `+[AVCaptureDevice authorizationStatusForMediaType:]` with
/// `AVMediaTypeAudio`. Safe to call from any thread; never panics.
pub fn status() -> MicPermission {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    // SAFETY: `authorizationStatusForMediaType:` is a read-only class method
    // that returns a plain integer enum. It is safe to call from any thread
    // and does not mutate any shared state. `AVMediaTypeAudio` is a
    // statically-initialized NSString constant provided by the framework.
    let av_status = unsafe {
        let media_type =
            AVMediaTypeAudio.expect("AVMediaTypeAudio static is always present on macOS");
        AVCaptureDevice::authorizationStatusForMediaType(media_type)
    };

    match av_status {
        AVAuthorizationStatus::Authorized => MicPermission::Granted,
        AVAuthorizationStatus::Denied => MicPermission::Denied,
        AVAuthorizationStatus::Restricted => MicPermission::Restricted,
        AVAuthorizationStatus::NotDetermined => MicPermission::NotDetermined,
        // Any unknown future variant → treat as not determined (most permissive)
        _ => MicPermission::NotDetermined,
    }
}

/// Requests microphone access from macOS and returns the resulting OS status.
///
/// If access has already been decided, AVFoundation will not show a prompt; we
/// return the current authorization status. Only `NotDetermined` triggers the
/// native permission sheet.
pub fn request_access() -> MicPermission {
    let current = status();
    if current != MicPermission::NotDetermined {
        return current;
    }

    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};

    let (tx, rx) = mpsc::channel();
    let handler = block2::RcBlock::new(move |granted: Bool| {
        let _ = tx.send(granted.as_bool());
    });

    // SAFETY: `requestAccessForMediaType:completionHandler:` is the documented
    // AVFoundation API for requesting microphone access. `AVMediaTypeAudio` is a
    // static framework constant, and `handler` is kept alive until the callback
    // resolves through the channel below.
    unsafe {
        let media_type =
            AVMediaTypeAudio.expect("AVMediaTypeAudio static is always present on macOS");
        AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &handler);
    }

    match rx.recv() {
        Ok(true) => MicPermission::Granted,
        Ok(false) => status(),
        Err(_) => status(),
    }
}

/// Opens System Settings › Privacy & Security › Microphone.
pub fn open_privacy_pane() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        .spawn();
}

/// Best-effort: if the user has been denied, open the privacy pane so they
/// can re-grant. Otherwise this is a no-op stub — cpal opening an input
/// stream (Task 3.3) will trigger the system prompt automatically when the
/// status is `NotDetermined`.
#[allow(dead_code)]
pub fn request_via_prompt() {
    if status() == MicPermission::Denied {
        open_privacy_pane();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `status()` must be total — it must return a valid variant without
    /// panicking on any macOS host (granted, denied, not-determined, etc.).
    #[test]
    fn status_is_total_and_does_not_panic() {
        let s = status();
        // All arms are reachable; just assert we got one of the four variants.
        assert!(
            matches!(
                s,
                MicPermission::Granted
                    | MicPermission::Denied
                    | MicPermission::NotDetermined
                    | MicPermission::Restricted
            ),
            "status() returned an unexpected value: {s:?}"
        );
    }
}

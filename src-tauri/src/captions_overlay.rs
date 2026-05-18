//! Make the captions window a true overlay over another app's macOS fullscreen.
//!
//! Background. The captions window is a transparent, decorless Tauri
//! `WebviewWindow`. `WebviewWindowBuilder::always_on_top(true)` only raises the
//! AppKit window *level* (`NSFloatingWindowLevel`), and Tauri's
//! `set_visible_on_all_workspaces(true)` only adds
//! `NSWindowCollectionBehaviorCanJoinAllSpaces` (verified in tao 0.35's
//! `set_visible_on_all_workspaces`). Neither makes the window appear over
//! *another* application that is in macOS **native fullscreen** — a fullscreen
//! app gets its own dedicated Space, and a foreign window is only allowed into
//! it when it also has `NSWindowCollectionBehaviorFullScreenAuxiliary`.
//!
//! There is no portable Tauri API for `FullScreenAuxiliary`, so we set the
//! collection behavior on the underlying `NSWindow` directly. macOS honours
//! this for an ordinary app window with no extra entitlement; if a future macOS
//! release ever blocks it, it fails honestly (the window simply will not enter
//! the fullscreen Space) rather than via a half-working fallback.
//!
//! The numeric mask is kept in [`overlay_collection_behavior`] so the exact
//! value is unit-testable on any platform without a GUI or AppKit.

/// `NSWindowCollectionBehaviorCanJoinAllSpaces` — present on every Space.
const CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
/// `NSWindowCollectionBehaviorStationary` — do not slide during Space swipes;
/// the overlay should hold its place while the user moves between Spaces.
const STATIONARY: u64 = 1 << 4;
/// `NSWindowCollectionBehaviorFullScreenAuxiliary` — allowed to float above
/// another app that is in native fullscreen. This is the bit Tauri/tao never
/// sets and the reason captions previously vanished over fullscreen meetings.
const FULL_SCREEN_AUXILIARY: u64 = 1 << 8;

/// The exact `NSWindowCollectionBehavior` bitmask applied to the captions
/// window. Pure and dependency-free so it can be asserted in unit tests.
pub fn overlay_collection_behavior() -> u64 {
    CAN_JOIN_ALL_SPACES | STATIONARY | FULL_SCREEN_AUXILIARY
}

/// Apply the fullscreen-overlay collection behavior to the captions window.
///
/// Safe to call repeatedly (idempotent) and safe to call when the window does
/// not exist (no-op). All AppKit mutation is dispatched to the main thread.
#[cfg(target_os = "macos")]
pub fn apply_overlay_behavior(app: &tauri::AppHandle) {
    use tauri::Manager as _;

    let Some(window) = app.get_webview_window("captions") else {
        return;
    };
    let win = window.clone();
    let _ = window.run_on_main_thread(move || {
        let ptr = match win.ns_window() {
            Ok(ptr) if !ptr.is_null() => ptr,
            _ => return,
        };
        use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
        // SAFETY: `ptr` is the live `NSWindow` backing this Tauri window, we
        // are on the main thread (`NSWindow` is `MainThreadOnly`), and we only
        // hold the reference for the duration of this call.
        let ns_window: &NSWindow = unsafe { &*ptr.cast::<NSWindow>() };
        let behavior =
            NSWindowCollectionBehavior::from_bits_retain(overlay_collection_behavior() as usize);
        ns_window.setCollectionBehavior(behavior);
    });
}

#[cfg(not(target_os = "macos"))]
pub fn apply_overlay_behavior(_app: &tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_is_the_three_expected_bits() {
        // CanJoinAllSpaces (1) | Stationary (16) | FullScreenAuxiliary (256).
        assert_eq!(overlay_collection_behavior(), 1 | 16 | 256);
    }

    #[test]
    fn mask_includes_fullscreen_auxiliary() {
        // The whole point of this module: without this bit the captions window
        // cannot appear over another app's native fullscreen Space.
        assert_ne!(overlay_collection_behavior() & FULL_SCREEN_AUXILIARY, 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn literals_match_appkit_named_constants() {
        // Guard against an objc2-app-kit bump silently changing the bit values
        // out from under our hand-written mask.
        use objc2_app_kit::NSWindowCollectionBehavior as B;
        let named = B::CanJoinAllSpaces | B::Stationary | B::FullScreenAuxiliary;
        assert_eq!(named.bits() as u64, overlay_collection_behavior());
    }
}

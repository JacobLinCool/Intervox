//! Centralized desktop-notification scheduling/dispatch (issue #2).
//!
//! All reminder notification logic lives here so future reminders don't get
//! scattered through engine/UI code:
//!
//! * [`reminder`] is the pure, unit-tested state machine (duration milestones
//!   + inactivity + de-duplication).
//! * [`init`] starts two background tasks: a one-shot non-blocking permission
//!   probe, and a coarse periodic ticker that feeds the tracker an engine
//!   [`ReminderSnapshot`](reminder::ReminderSnapshot) and turns the returned
//!   reminders into **silent** OS notifications.
//!
//! Notifications are intentionally silent (`NotificationBuilder::silent()`):
//! the banner is shown but no alert sound is played.

pub mod reminder;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::{NotificationExt, PermissionState};

use crate::engine::Engine;
use reminder::{ReminderConfig, ReminderTracker};

/// How often the dispatcher re-evaluates reminder state. Coarse on purpose:
/// the tracker de-duplicates, so a slow tick is cheap and robust across
/// sleep/wake and status refreshes.
const TICK: Duration = Duration::from_secs(15);

/// Event emitted to the frontend whenever the notification-permission state
/// is first determined or changes, so the UI can surface a denied state.
pub const PERMISSION_EVENT: &str = "notification-permission-changed";

/// Notification permission as exposed to the frontend. `Unsupported` covers
/// "the platform/plugin couldn't tell us" — no silent fallback path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationPermission {
    Granted,
    Denied,
    /// Not yet decided (will be requested) — also the honest initial value
    /// before the async probe completes.
    Prompt,
    Unsupported,
}

impl From<PermissionState> for NotificationPermission {
    fn from(state: PermissionState) -> Self {
        match state {
            PermissionState::Granted => Self::Granted,
            PermissionState::Denied => Self::Denied,
            // Prompt / PromptWithRationale → still undecided.
            _ => Self::Prompt,
        }
    }
}

/// Managed state holding the last known permission. `None` until the async
/// startup probe resolves it.
#[derive(Default)]
pub struct NotificationStatusState(pub Mutex<Option<NotificationPermission>>);

/// Payload of the `get_notification_status` command.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationStatus {
    pub permission: NotificationPermission,
    /// Configured inactivity reminder period in minutes; `0` means disabled.
    pub inactivity_minutes: u32,
}

fn inactivity_threshold_from_minutes(minutes: u32) -> Option<Duration> {
    if minutes == 0 {
        None
    } else {
        Some(Duration::from_secs(u64::from(minutes) * 60))
    }
}

fn read_inactivity_minutes(app: &AppHandle) -> u32 {
    app.state::<crate::commands::AppHandle>()
        .config
        .lock()
        .map(|c| c.ui.inactivity_reminder_minutes)
        .unwrap_or(0)
}

/// Update the cached permission and emit the change event when it actually
/// changes (so the frontend can react without polling).
fn set_permission(app: &AppHandle, perm: NotificationPermission) {
    let changed = {
        let st = app.state::<NotificationStatusState>();
        let mut guard = st.0.lock().expect("notification permission lock");
        let changed = *guard != Some(perm);
        *guard = Some(perm);
        changed
    };
    if changed {
        let _ = app.emit(PERMISSION_EVENT, perm);
    }
}

/// Show a silent desktop notification. Silent = visible banner, no sound
/// (issue #2). A failed `show()` is swallowed: a missed reminder must never
/// break Interpret.
fn show_silent(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .silent()
        .show()
    {
        eprintln!("[intervox:notifications] failed to show notification: {e}");
    }
}

/// Start notification handling. Must be called once from the Tauri `setup`
/// hook after the engine is managed. Spawns only background tasks — it never
/// blocks app startup, and notification permission is requested off the
/// startup path.
pub fn init(app: &AppHandle, engine: Arc<Engine>) {
    app.manage(NotificationStatusState::default());

    // ── 1. Permission probe (non-blocking) ────────────────────────────────
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let perm = match app.notification().permission_state() {
                Ok(PermissionState::Granted) => NotificationPermission::Granted,
                Ok(PermissionState::Denied) => NotificationPermission::Denied,
                // Undecided → request once. On desktop this resolves
                // immediately; it never gates startup since we're detached.
                Ok(_) => app
                    .notification()
                    .request_permission()
                    .map(NotificationPermission::from)
                    .unwrap_or(NotificationPermission::Unsupported),
                Err(_) => NotificationPermission::Unsupported,
            };
            set_permission(&app, perm);
        });
    }

    // ── 2. Reminder ticker ────────────────────────────────────────────────
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut tracker = ReminderTracker::new(ReminderConfig::new(
            inactivity_threshold_from_minutes(read_inactivity_minutes(&app)),
        ));
        let mut tick = tokio::time::interval(TICK);
        // Drop the immediate first tick so we don't evaluate before the app
        // has settled.
        tick.tick().await;
        loop {
            tick.tick().await;
            // Pick up runtime changes to the configurable inactivity period.
            tracker.set_inactivity_threshold(inactivity_threshold_from_minutes(
                read_inactivity_minutes(&app),
            ));
            let snapshot = engine.reminder_snapshot();
            for reminder in tracker.observe(&snapshot) {
                let (title, body) = reminder.message();
                show_silent(&app, &title, &body);
            }
        }
    });
}

/// Frontend-facing snapshot of notification permission + configured period.
/// Used to surface a denied/unsupported state in Settings.
#[tauri::command]
pub fn get_notification_status(
    app: tauri::AppHandle,
    h: tauri::State<crate::commands::AppHandle>,
) -> NotificationStatus {
    let permission = app
        .state::<NotificationStatusState>()
        .0
        .lock()
        .ok()
        .and_then(|g| *g)
        // Before the async probe resolves, report "Prompt" (honest: not yet
        // determined) — never a fabricated Granted.
        .unwrap_or(NotificationPermission::Prompt);
    let inactivity_minutes = h
        .config
        .lock()
        .map(|c| c.ui.inactivity_reminder_minutes)
        .unwrap_or(0);
    NotificationStatus {
        permission,
        inactivity_minutes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_to_threshold_maps_zero_to_disabled() {
        assert_eq!(inactivity_threshold_from_minutes(0), None);
        assert_eq!(
            inactivity_threshold_from_minutes(10),
            Some(Duration::from_secs(600))
        );
        assert_eq!(
            inactivity_threshold_from_minutes(1),
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn permission_state_maps_to_frontend_enum() {
        assert_eq!(
            NotificationPermission::from(PermissionState::Granted),
            NotificationPermission::Granted
        );
        assert_eq!(
            NotificationPermission::from(PermissionState::Denied),
            NotificationPermission::Denied
        );
        assert_eq!(
            NotificationPermission::from(PermissionState::Prompt),
            NotificationPermission::Prompt
        );
    }
}

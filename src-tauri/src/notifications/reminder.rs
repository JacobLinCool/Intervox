//! Pure reminder state machine for desktop notifications.
//!
//! This module is intentionally free of any Tauri / OS dependency so the
//! reminder logic (duration milestones, inactivity, de-duplication) can be
//! unit-tested deterministically. The dispatcher in [`super`] feeds it
//! [`ReminderSnapshot`]s built from the engine's source-of-truth atomics and
//! turns the returned [`Reminder`]s into silent OS notifications.

use std::time::Duration;

/// Continuous-session duration milestones (issue #2: 1 h, 2 h, 3 h).
pub const DURATION_MILESTONES: [Duration; 3] = [
    Duration::from_secs(60 * 60),
    Duration::from_secs(2 * 60 * 60),
    Duration::from_secs(3 * 60 * 60),
];

/// Static configuration for the tracker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReminderConfig {
    /// Elapsed-since-session-start thresholds. Each fires at most once per
    /// continuous session.
    pub duration_milestones: Vec<Duration>,
    /// Inactivity threshold. `None` disables the inactivity reminder
    /// (e.g. the user set the configurable period to 0 / off).
    pub inactivity_threshold: Option<Duration>,
}

impl ReminderConfig {
    /// Default config: the issue's 1 h / 2 h / 3 h milestones plus the given
    /// (already validated) inactivity threshold, where `None` disables it.
    pub fn new(inactivity_threshold: Option<Duration>) -> Self {
        Self {
            duration_milestones: DURATION_MILESTONES.to_vec(),
            inactivity_threshold,
        }
    }
}

/// A single observation of the engine's reminder-relevant state.
///
/// Built once per tick from the engine's atomics so the tracker never reaches
/// into Tauri/engine internals directly.
#[derive(Clone, Debug)]
pub struct ReminderSnapshot {
    /// Identifies the current continuous Interpret/recording session.
    /// `None` when no session is active. A new session gets a new id, which
    /// is how the tracker knows to reset milestone/inactivity de-dup state
    /// after a mode toggle or session restart.
    pub session_id: Option<u64>,
    /// Time since the current session started. Meaningless (and ignored)
    /// when `session_id` is `None`.
    pub session_elapsed: Duration,
    /// Whether Interpret is enabled and the session is live. Drives the
    /// inactivity reminder; duration milestones only need an active session.
    pub interpret_active: bool,
    /// Time since the last user-visible interpreted text. Measured from the
    /// session start when no interpreted text has appeared yet, so a session
    /// that never produces output still trips the inactivity reminder.
    /// `None` when there is no active session.
    pub since_interpret_activity: Option<Duration>,
}

/// A reminder the dispatcher should surface as a silent OS notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reminder {
    /// The continuous session crossed `duration_milestones[index]`.
    DurationMilestone { index: usize, elapsed_secs: u64 },
    /// Interpret is on but no interpreted text has appeared for the
    /// configured threshold.
    Inactivity { idle_secs: u64 },
}

impl Reminder {
    /// User-facing `(title, body)` for the notification. Kept here so all
    /// reminder copy lives next to the state machine instead of being
    /// scattered through dispatch/UI code.
    pub fn message(&self) -> (String, String) {
        match self {
            Reminder::DurationMilestone { index, .. } => {
                let hours = index + 1;
                let unit = if hours == 1 { "hour" } else { "hours" };
                (
                    format!("Interpreting for {hours} {unit}"),
                    format!(
                        "Intervox has been interpreting continuously for {hours} {unit} and is still running."
                    ),
                )
            }
            Reminder::Inactivity { idle_secs } => {
                let minutes = (*idle_secs / 60).max(1);
                let unit = if minutes == 1 { "minute" } else { "minutes" };
                (
                    "No interpreted speech detected".to_string(),
                    format!(
                        "Interpret is on but nothing has been interpreted for about {minutes} {unit}. Check your audio source, or turn Interpret off if you're done."
                    ),
                )
            }
        }
    }
}

/// Stateful, per-session de-duplicating reminder tracker.
///
/// Call [`observe`](Self::observe) once per tick. It is the single place that
/// decides whether a reminder should fire, guaranteeing each milestone and the
/// inactivity reminder fire at most once per quiet stretch / session — which
/// is what makes the dispatcher safe to drive from a coarse periodic ticker
/// across status refreshes and sleep/wake.
pub struct ReminderTracker {
    config: ReminderConfig,
    /// The session this de-dup state belongs to. A change resets everything.
    session_id: Option<u64>,
    /// Per-milestone "already fired this session" flags, indexed like
    /// `config.duration_milestones`.
    fired_milestones: Vec<bool>,
    /// Whether the inactivity reminder already fired for the current quiet
    /// stretch. Cleared when interpreted text reappears, Interpret is
    /// disabled, or the session changes.
    inactivity_fired: bool,
}

impl ReminderTracker {
    pub fn new(config: ReminderConfig) -> Self {
        let n = config.duration_milestones.len();
        Self {
            config,
            session_id: None,
            fired_milestones: vec![false; n],
            inactivity_fired: false,
        }
    }

    /// Replace the inactivity threshold at runtime (the user can change the
    /// configurable period in Settings without restarting the app).
    pub fn set_inactivity_threshold(&mut self, threshold: Option<Duration>) {
        self.config.inactivity_threshold = threshold;
    }

    fn reset_session_state(&mut self) {
        for f in &mut self.fired_milestones {
            *f = false;
        }
        self.inactivity_fired = false;
    }

    /// Feed one observation; return any reminders that should fire now.
    pub fn observe(&mut self, snap: &ReminderSnapshot) -> Vec<Reminder> {
        // A changed session id (new session, restart, or back to no-session)
        // resets all per-session de-dup state. This is what prevents duplicate
        // milestones after mode toggles / session restarts.
        if snap.session_id != self.session_id {
            self.session_id = snap.session_id;
            self.reset_session_state();
        }

        // No active session → nothing to remind about. State is already clean
        // for the next session via the id-change reset above.
        if snap.session_id.is_none() {
            return Vec::new();
        }

        let mut out = Vec::new();

        // ── Duration milestones ───────────────────────────────────────────
        // Mark every crossed-but-unfired milestone as fired, but only surface
        // the highest one. Under normal coarse ticking each tick crosses at
        // most one milestone (one notification each). If the machine slept
        // across several thresholds we collapse them into a single "N hours"
        // notification instead of a burst — still no duplicates.
        let mut highest: Option<usize> = None;
        for (i, threshold) in self.config.duration_milestones.iter().enumerate() {
            if !self.fired_milestones[i] && snap.session_elapsed >= *threshold {
                self.fired_milestones[i] = true;
                highest = Some(i);
            }
        }
        if let Some(i) = highest {
            out.push(Reminder::DurationMilestone {
                index: i,
                elapsed_secs: snap.session_elapsed.as_secs(),
            });
        }

        // ── Inactivity ────────────────────────────────────────────────────
        match (
            snap.interpret_active,
            self.config.inactivity_threshold,
            snap.since_interpret_activity,
        ) {
            // Interpret disabled → reset so a future quiet stretch can fire.
            (false, _, _) => self.inactivity_fired = false,
            (true, Some(threshold), Some(since)) => {
                if since >= threshold {
                    if !self.inactivity_fired {
                        self.inactivity_fired = true;
                        out.push(Reminder::Inactivity {
                            idle_secs: since.as_secs(),
                        });
                    }
                } else {
                    // Interpreted text reappeared (or never lapsed): re-arm so
                    // the next quiet stretch reminds again.
                    self.inactivity_fired = false;
                }
            }
            // Inactivity disabled, or no activity figure available.
            _ => {}
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(inactivity_secs: Option<u64>) -> ReminderConfig {
        ReminderConfig::new(inactivity_secs.map(Duration::from_secs))
    }

    fn snap(
        session_id: Option<u64>,
        elapsed_secs: u64,
        interpret_active: bool,
        since_secs: Option<u64>,
    ) -> ReminderSnapshot {
        ReminderSnapshot {
            session_id,
            session_elapsed: Duration::from_secs(elapsed_secs),
            interpret_active,
            since_interpret_activity: since_secs.map(Duration::from_secs),
        }
    }

    const H: u64 = 3600;

    #[test]
    fn fires_each_duration_milestone_once_in_order() {
        let mut t = ReminderTracker::new(cfg(None));
        // Before 1h: nothing.
        assert!(t.observe(&snap(Some(1), H - 1, true, Some(0))).is_empty());
        // Cross 1h.
        assert_eq!(
            t.observe(&snap(Some(1), H, true, Some(0))),
            vec![Reminder::DurationMilestone {
                index: 0,
                elapsed_secs: H
            }]
        );
        // 2h.
        assert_eq!(
            t.observe(&snap(Some(1), 2 * H, true, Some(0))),
            vec![Reminder::DurationMilestone {
                index: 1,
                elapsed_secs: 2 * H
            }]
        );
        // 3h.
        assert_eq!(
            t.observe(&snap(Some(1), 3 * H, true, Some(0))),
            vec![Reminder::DurationMilestone {
                index: 2,
                elapsed_secs: 3 * H
            }]
        );
    }

    #[test]
    fn duration_milestone_does_not_duplicate_across_repeated_ticks() {
        let mut t = ReminderTracker::new(cfg(None));
        assert_eq!(t.observe(&snap(Some(1), H, true, Some(0))).len(), 1);
        // Many subsequent ticks (status refreshes) must not re-fire 1h.
        for extra in 1..50 {
            assert!(t
                .observe(&snap(Some(1), H + extra, true, Some(0)))
                .is_empty());
        }
    }

    #[test]
    fn sleep_wake_jump_collapses_to_single_highest_notification() {
        let mut t = ReminderTracker::new(cfg(None));
        // Machine slept; we jump straight past all three thresholds in one tick.
        let fired = t.observe(&snap(Some(1), 3 * H + 120, true, Some(0)));
        assert_eq!(
            fired,
            vec![Reminder::DurationMilestone {
                index: 2,
                elapsed_secs: 3 * H + 120
            }]
        );
        // And it must not re-fire afterwards.
        assert!(t.observe(&snap(Some(1), 4 * H, true, Some(0))).is_empty());
    }

    #[test]
    fn new_session_resets_milestones_no_duplicate_after_mode_toggle() {
        let mut t = ReminderTracker::new(cfg(None));
        assert_eq!(t.observe(&snap(Some(1), H, true, Some(0))).len(), 1);
        // Mode toggled off: session id goes None (no duplicate, nothing fires).
        assert!(t.observe(&snap(None, 0, false, None)).is_empty());
        // Toggled back on: brand new session (id 2). 1h fires again — this is
        // a genuinely new continuous session, not a duplicate of the old one.
        assert!(t.observe(&snap(Some(2), H - 5, true, Some(0))).is_empty());
        assert_eq!(
            t.observe(&snap(Some(2), H, true, Some(0))),
            vec![Reminder::DurationMilestone {
                index: 0,
                elapsed_secs: H
            }]
        );
    }

    #[test]
    fn no_session_never_fires() {
        let mut t = ReminderTracker::new(cfg(Some(600)));
        for _ in 0..10 {
            assert!(t.observe(&snap(None, 99 * H, false, None)).is_empty());
        }
    }

    #[test]
    fn inactivity_fires_once_then_rearms_when_text_reappears() {
        let mut t = ReminderTracker::new(cfg(Some(600))); // 10 min
        // Under threshold: silent.
        assert!(t.observe(&snap(Some(1), 60, true, Some(599))).is_empty());
        // Cross threshold: fire once.
        assert_eq!(
            t.observe(&snap(Some(1), 700, true, Some(600))),
            vec![Reminder::Inactivity { idle_secs: 600 }]
        );
        // Still quiet: no duplicate.
        assert!(t.observe(&snap(Some(1), 900, true, Some(900))).is_empty());
        // Interpreted text reappears (since resets below threshold): re-arm.
        assert!(t.observe(&snap(Some(1), 905, true, Some(2))).is_empty());
        // Quiet again past threshold: fires a second time (new quiet stretch).
        assert_eq!(
            t.observe(&snap(Some(1), 1600, true, Some(601))),
            vec![Reminder::Inactivity { idle_secs: 601 }]
        );
    }

    #[test]
    fn inactivity_resets_when_interpret_disabled() {
        let mut t = ReminderTracker::new(cfg(Some(300)));
        assert_eq!(
            t.observe(&snap(Some(1), 400, true, Some(300))).len(),
            1
        );
        // Interpret disabled mid-session (same id) → re-arm.
        assert!(t
            .observe(&snap(Some(1), 410, false, Some(310)))
            .is_empty());
        // Back on and quiet again → fires again.
        assert_eq!(
            t.observe(&snap(Some(1), 800, true, Some(700))),
            vec![Reminder::Inactivity { idle_secs: 700 }]
        );
    }

    #[test]
    fn inactivity_disabled_when_threshold_none() {
        let mut t = ReminderTracker::new(cfg(None));
        // Elapsed kept below the first (1 h) duration milestone so this test
        // isolates the inactivity path: with threshold None nothing fires no
        // matter how long the quiet stretch is.
        for s in 0..10 {
            assert!(t
                .observe(&snap(Some(1), 60, true, Some(s * 600)))
                .is_empty());
        }
    }

    #[test]
    fn runtime_threshold_change_takes_effect() {
        let mut t = ReminderTracker::new(cfg(None));
        assert!(t.observe(&snap(Some(1), 100, true, Some(99))).is_empty());
        t.set_inactivity_threshold(Some(Duration::from_secs(60)));
        assert_eq!(
            t.observe(&snap(Some(1), 200, true, Some(120))),
            vec![Reminder::Inactivity { idle_secs: 120 }]
        );
    }

    #[test]
    fn duration_and_inactivity_can_fire_in_same_tick() {
        let mut t = ReminderTracker::new(cfg(Some(600)));
        let fired = t.observe(&snap(Some(1), H, true, Some(600)));
        assert!(fired.contains(&Reminder::DurationMilestone {
            index: 0,
            elapsed_secs: H
        }));
        assert!(fired.contains(&Reminder::Inactivity { idle_secs: 600 }));
    }

    #[test]
    fn message_copy_is_singular_and_plural_correct() {
        let (t1, _) = (Reminder::DurationMilestone {
            index: 0,
            elapsed_secs: H,
        })
        .message();
        assert_eq!(t1, "Interpreting for 1 hour");
        let (t2, _) = (Reminder::DurationMilestone {
            index: 1,
            elapsed_secs: 2 * H,
        })
        .message();
        assert_eq!(t2, "Interpreting for 2 hours");
        let (_, b) = (Reminder::Inactivity { idle_secs: 60 }).message();
        assert!(b.contains("1 minute"), "got: {b}");
        let (_, b2) = (Reminder::Inactivity { idle_secs: 630 }).message();
        assert!(b2.contains("10 minutes"), "got: {b2}");
    }
}

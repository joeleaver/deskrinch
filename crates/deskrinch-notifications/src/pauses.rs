//! Why a notification's expiry is held, for the service's one pause.
//!
//! The service pauses a notification or not (`Handle::pause`,
//! `Handle::resume`); the daemon has two reasons to: the pointer is over
//! its popup, or it is waiting (not shown, so its timeout must not run).
//! [`Pauses`] keeps each notification's reasons and says when the service
//! must be told: pause when the first reason comes, resume when the last
//! goes. So a popup pushed out of the column while under the pointer stays
//! paused, and a pointer leaving a popup that is now waiting does not
//! resume it.
//!
//! The service's pause leaves a notification that never expires as it is,
//! and keeps a pause through a replacement only if there was one: so one
//! held while it had no timeout, given one by a replacement, would start
//! its timer. The daemon pauses again every notification held
//! ([`Pauses::held`]) when the live ones change.

use std::collections::BTreeMap;

use deskrinch_notification_service::NotificationId;

/// A reason to hold a notification's expiry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Reason {
    /// The pointer is over its popup.
    Pointer,
    /// It is waiting, not shown.
    Waiting,
}

/// What the service must be told about a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tell {
    /// Its first reason came: pause it.
    Pause,
    /// Its last reason went: resume it.
    Resume,
    /// Nothing changed for the service.
    Nothing,
}

/// Each notification's reasons to be held.
#[derive(Debug, Default)]
pub(crate) struct Pauses {
    reasons: BTreeMap<NotificationId, (bool, bool)>,
}

impl Pauses {
    /// `reason` now holds `id` (`on`) or no longer does; what to tell the
    /// service.
    pub(crate) fn set(&mut self, id: NotificationId, reason: Reason, on: bool) -> Tell {
        let entry = self.reasons.entry(id).or_default();
        let before = entry.0 || entry.1;
        match reason {
            Reason::Pointer => entry.0 = on,
            Reason::Waiting => entry.1 = on,
        }
        let after = entry.0 || entry.1;
        if !after {
            self.reasons.remove(&id);
        }
        match (before, after) {
            (false, true) => Tell::Pause,
            (true, false) => Tell::Resume,
            _ => Tell::Nothing,
        }
    }

    /// The notifications something holds.
    pub(crate) fn held(&self) -> Vec<NotificationId> {
        self.reasons.keys().copied().collect()
    }

    /// Only the notifications in `live` are live: the others' reasons are
    /// forgotten, and nothing is told (a closed one has nothing to resume).
    pub(crate) fn retain(&mut self, live: &[NotificationId]) {
        self.reasons.retain(|id, _| live.contains(id));
    }

    /// Whether `reason` holds `id`.
    #[cfg(test)]
    fn holds(&self, id: NotificationId, reason: Reason) -> bool {
        self.reasons
            .get(&id)
            .is_some_and(|(pointer, waiting)| match reason {
                Reason::Pointer => *pointer,
                Reason::Waiting => *waiting,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u32) -> NotificationId {
        NotificationId::new(n).unwrap()
    }

    #[test]
    fn the_first_reason_pauses_and_the_last_resumes() {
        let mut pauses = Pauses::default();
        assert_eq!(pauses.set(id(1), Reason::Pointer, true), Tell::Pause);
        assert_eq!(pauses.set(id(1), Reason::Pointer, false), Tell::Resume);
    }

    #[test]
    fn pushed_out_under_the_pointer_it_stays_paused() {
        let mut pauses = Pauses::default();
        assert_eq!(pauses.set(id(1), Reason::Pointer, true), Tell::Pause);
        assert_eq!(pauses.set(id(1), Reason::Waiting, true), Tell::Nothing);
        // The pointer's leave, late, does not resume a waiting one.
        assert_eq!(pauses.set(id(1), Reason::Pointer, false), Tell::Nothing);
        assert!(pauses.holds(id(1), Reason::Waiting));
        // Shown again: its timeout runs again.
        assert_eq!(pauses.set(id(1), Reason::Waiting, false), Tell::Resume);
    }

    #[test]
    fn a_reason_said_twice_tells_once() {
        let mut pauses = Pauses::default();
        assert_eq!(pauses.set(id(1), Reason::Waiting, true), Tell::Pause);
        assert_eq!(pauses.set(id(1), Reason::Waiting, true), Tell::Nothing);
        assert_eq!(pauses.set(id(2), Reason::Pointer, false), Tell::Nothing);
    }

    #[test]
    fn a_closed_notification_is_forgotten() {
        let mut pauses = Pauses::default();
        pauses.set(id(1), Reason::Waiting, true);
        pauses.set(id(2), Reason::Waiting, true);
        pauses.retain(&[id(2)]);
        assert!(!pauses.holds(id(1), Reason::Waiting));
        assert!(pauses.holds(id(2), Reason::Waiting));
        assert_eq!(pauses.set(id(1), Reason::Pointer, true), Tell::Pause);
    }
}

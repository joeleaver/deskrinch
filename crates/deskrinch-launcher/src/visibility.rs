//! Whether the launcher is shown: one value, changed from the bus and from
//! the launcher's own UI, and followed by its window.

use std::sync::{Mutex, PoisonError};

use deskrinch_component::Handover;
use rinch::prelude::Signal;

/// Whether the launcher is shown, and the hand-over that tells its window.
///
/// Every change goes through here, whoever makes it: the bus's `Toggle`,
/// `Show` and `Hide` on the bus's thread, and the UI's own hiding (Escape,
/// a start) on the UI thread. So a `Toggle` always flips what the window
/// was last asked to be, however it got there.
#[derive(Debug)]
pub struct Visibility {
    shown: Mutex<bool>,
    window: Handover<bool>,
}

impl Visibility {
    /// Hidden or shown at first, as `shown` says.
    #[must_use]
    pub fn new(shown: bool) -> Self {
        Self {
            shown: Mutex::new(shown),
            window: Handover::default(),
        }
    }

    /// Whether the launcher is shown, or about to be.
    #[must_use]
    pub fn is_shown(&self) -> bool {
        *self.lock()
    }

    /// Shows it if hidden, hides it if shown; answers whether it is now
    /// shown.
    pub fn toggle(&self) -> bool {
        let mut shown = self.lock();
        *shown = !*shown;
        self.window.deliver(*shown);
        *shown
    }

    /// Shows it (already shown: nothing changes).
    pub fn show(&self) {
        self.set(true);
    }

    /// Hides it (already hidden: nothing changes).
    pub fn hide(&self) {
        self.set(false);
    }

    /// The window's signal, on the UI thread, once: whether it should be
    /// shown, from now on.
    pub fn signal(&self) -> Signal<bool> {
        let shown = self.is_shown();
        self.window.signal(shown)
    }

    fn set(&self, wanted: bool) {
        let mut shown = self.lock();
        if *shown != wanted {
            *shown = wanted;
            self.window.deliver(wanted);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, bool> {
        // A panic while holding the lock leaves a valid bool.
        self.shown.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_flips_and_show_and_hide_set() {
        let visibility = Visibility::new(false);
        assert!(visibility.toggle());
        assert!(visibility.is_shown());
        assert!(!visibility.toggle());
        visibility.show();
        visibility.show();
        assert!(visibility.is_shown());
        visibility.hide();
        assert!(!visibility.is_shown());
        // A toggle after the UI hid it shows it again.
        visibility.show();
        visibility.hide();
        assert!(visibility.toggle());
    }

    #[test]
    fn starts_as_asked() {
        assert!(Visibility::new(true).is_shown());
        assert!(!Visibility::new(false).is_shown());
    }
}

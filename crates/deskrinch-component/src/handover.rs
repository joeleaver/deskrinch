//! Values from another thread to the UI, before and after the UI exists.

use std::fmt;
use std::sync::{Mutex, PoisonError};

use rinch::prelude::Signal;

/// Where a thread of the component's (a configuration watcher, a D-Bus
/// client) hands values to the UI.
///
/// Until the UI's signal exists the newest value is held;
/// [`signal`](Self::signal), on the UI thread, makes the signal from it,
/// and every later [`deliver`](Self::deliver) is sent to the signal with
/// [`Signal::send`], rinch's write from another thread.
pub struct Handover<T: 'static>(Mutex<Target<T>>);

enum Target<T: 'static> {
    /// No signal yet; the newest value so far.
    Waiting(Option<T>),
    /// The UI's signal.
    Connected(Signal<T>),
}

impl<T> Default for Handover<T> {
    fn default() -> Self {
        Self(Mutex::new(Target::Waiting(None)))
    }
}

impl<T: Send + Clone> Handover<T> {
    /// Hands `value` to the UI, from any thread.
    pub fn deliver(&self, value: T) {
        // A panic while holding the lock leaves a valid `Target`.
        let mut target = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match &mut *target {
            Target::Waiting(pending) => *pending = Some(value),
            Target::Connected(signal) => signal.send(value),
        }
    }

    /// The UI's signal, on the UI thread: the newest value delivered so far,
    /// or `initial` when there is none, and every later one. Call it once;
    /// a second call makes a signal that nothing is delivered to.
    pub fn signal(&self, initial: T) -> Signal<T> {
        let signal = Signal::new(initial);
        let mut target = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Target::Waiting(pending) = std::mem::replace(&mut *target, Target::Connected(signal))
            && let Some(newer) = pending
        {
            signal.set(newer);
        }
        signal
    }

    /// The value held for a UI that does not exist yet.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> Option<T> {
        match &*self.0.lock().unwrap_or_else(PoisonError::into_inner) {
            Target::Waiting(pending) => pending.clone(),
            Target::Connected(_) => None,
        }
    }
}

impl<T> fmt::Debug for Handover<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let connected = matches!(
            &*self.0.lock().unwrap_or_else(PoisonError::into_inner),
            Target::Connected(_)
        );
        f.debug_struct("Handover")
            .field("connected", &connected)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_the_newest_value_until_the_ui_connects_then_sends() {
        let handover = Handover::default();
        handover.deliver(1);
        handover.deliver(2);
        assert_eq!(handover.pending(), Some(2));
        let signal = handover.signal(0);
        assert_eq!(signal.get(), 2);
        assert_eq!(handover.pending(), None);
        handover.deliver(3);
        assert_eq!(signal.get(), 3);
    }

    #[test]
    fn with_nothing_delivered_the_signal_starts_at_the_initial_value() {
        let handover = Handover::<u32>::default();
        assert_eq!(handover.signal(7).get(), 7);
    }
}

//! Values from another thread to the UI, before and after the UI exists.
//!
//! A hand-over is made as two halves that share one state, as a channel's
//! are ([`handover`]): a [`Handover`], which delivers values from any
//! thread and is cloned for every thread that delivers; and an [`Inbox`],
//! the UI's, which is consumed to make the UI's signal
//! ([`Inbox::into_signal`]). There is one inbox, and making the signal uses
//! it up, so a hand-over has exactly one signal: a second is not a mistake
//! made at run time but code that does not compile.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use rinch::prelude::Signal;

/// A new hand-over: the half that delivers, and the half the UI turns into
/// its signal.
#[must_use]
pub fn handover<T: 'static>() -> (Handover<T>, Inbox<T>) {
    let shared = Arc::new(Mutex::new(Target::Waiting(None)));
    (Handover(Arc::clone(&shared)), Inbox(shared))
}

/// Where a thread of the component's (a configuration watcher, a D-Bus
/// client) hands values to the UI: [`deliver`](Self::deliver), from any
/// thread. Until the UI's signal exists the newest value is held; after,
/// every value is sent to the signal with [`Signal::send`], rinch's write
/// from another thread.
pub struct Handover<T: 'static>(Arc<Mutex<Target<T>>>);

/// The UI's half of a hand-over, from [`handover`]: turned into the UI's
/// signal once, on the UI thread ([`into_signal`](Self::into_signal)).
///
/// The signal belongs to the reactive scope that makes it, and lives as
/// long as that scope: deliveries after the scope is disposed go nowhere.
/// So it is made in the component's root, whose scope lasts as long as the
/// component, and shared from there (through the context, say); not in a
/// widget or a window that may close while the component runs on.
///
/// It is used once:
///
/// ```compile_fail,E0382
/// let (_deliver, inbox) = deskrinch_component::handover::<u32>();
/// let first = inbox.into_signal(0);
/// // `inbox` was moved by the first call.
/// let second = inbox.into_signal(0);
/// ```
pub struct Inbox<T: 'static>(Arc<Mutex<Target<T>>>);

enum Target<T: 'static> {
    /// No signal yet; the newest value so far.
    Waiting(Option<T>),
    /// The UI's signal.
    Connected(Signal<T>),
}

impl<T> Clone for Handover<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
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

    /// The value held for a UI that does not exist yet.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> Option<T> {
        pending(&self.0)
    }
}

impl<T: Send + Clone> Inbox<T> {
    /// The value held for the UI, before it makes its signal: for the
    /// tests, which have no UI.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> Option<T> {
        pending(&self.0)
    }

    /// The UI's signal, on the UI thread: the newest value delivered so far,
    /// or `initial` when there is none, and every later one.
    pub fn into_signal(self, initial: T) -> Signal<T> {
        let signal = Signal::new(initial);
        let mut target = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Target::Waiting(pending) = std::mem::replace(&mut *target, Target::Connected(signal))
            && let Some(newer) = pending
        {
            signal.set(newer);
        }
        signal
    }
}

#[cfg(test)]
fn pending<T: Clone>(target: &Mutex<Target<T>>) -> Option<T> {
    match &*target.lock().unwrap_or_else(PoisonError::into_inner) {
        Target::Waiting(pending) => pending.clone(),
        Target::Connected(_) => None,
    }
}

fn connected<T>(target: &Mutex<Target<T>>) -> bool {
    matches!(
        &*target.lock().unwrap_or_else(PoisonError::into_inner),
        Target::Connected(_)
    )
}

impl<T> fmt::Debug for Handover<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handover")
            .field("connected", &connected(&self.0))
            .finish_non_exhaustive()
    }
}

impl<T> fmt::Debug for Inbox<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inbox").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_the_newest_value_until_the_ui_connects_then_sends() {
        let (handover, inbox) = handover();
        handover.deliver(1);
        handover.deliver(2);
        assert_eq!(handover.pending(), Some(2));
        let signal = inbox.into_signal(0);
        assert_eq!(signal.get(), 2);
        assert_eq!(handover.pending(), None);
    }

    #[test]
    fn with_nothing_delivered_the_signal_starts_at_the_initial_value() {
        let (_handover, inbox) = handover::<u32>();
        assert_eq!(inbox.into_signal(7).get(), 7);
    }

    #[test]
    fn a_later_delivery_reaches_the_signal_from_every_clone() {
        let (handover, inbox) = handover();
        let signal = inbox.into_signal(0);
        handover.deliver(3);
        assert_eq!(signal.get(), 3);
        let other = handover.clone();
        other.deliver(4);
        assert_eq!(signal.get(), 4);
    }
}

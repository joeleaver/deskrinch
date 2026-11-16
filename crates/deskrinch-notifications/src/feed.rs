//! The service's events, handed to the UI as the whole state each time:
//! the live notifications, in the model's order, and whether the service
//! has ended.
//!
//! The subscriber runs on a thread of the service's. At each event it
//! takes the model's list from the [`Handle`] and hands it over
//! ([`Handover`]); the UI keeps no list of its own, so the model's order
//! and contents are the only ones. The handle exists only once the service
//! has started, and a sender may be quick: a notification can arrive before
//! it. So the subscriber and [`Feed::started`] take the list and hand it
//! over under one lock, and the last list handed over is always the newest
//! taken.

use std::sync::{Arc, Mutex, PoisonError};

use deskrinch_component::Handover;
use deskrinch_notification_service::{Event, Handle, Notification};
use rinch::prelude::Signal;

/// What the UI is shown.
#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    /// The live notifications, in the model's order (first shown first).
    pub(crate) live: Arc<Vec<Arc<Notification>>>,
    /// The service ended (its connection, or its name, is gone): the daemon
    /// ends too, with a failure.
    pub(crate) ended: bool,
}

/// Where the service's subscriber hands the state to the UI.
#[derive(Debug, Default)]
pub(crate) struct Feed {
    inner: Mutex<Inner>,
    handover: Handover<State>,
}

#[derive(Debug, Default)]
struct Inner {
    handle: Option<Handle>,
    ended: bool,
}

impl Feed {
    /// The subscriber's work for `event`, on the service's thread.
    pub(crate) fn event(&self, event: &Event) {
        let mut inner = self.lock();
        match event {
            Event::Disconnected => {
                tracing::error!("the session bus connection ended; the notification daemon ends");
                inner.ended = true;
            }
            Event::NameLost => {
                tracing::error!(
                    "another program took org.freedesktop.Notifications; the notification \
                     daemon ends"
                );
                inner.ended = true;
            }
            _ => {}
        }
        self.hand_over(&inner);
    }

    /// The service has started, and acts through `handle`.
    pub(crate) fn started(&self, handle: Handle) {
        let mut inner = self.lock();
        inner.handle = Some(handle);
        self.hand_over(&inner);
    }

    /// The UI's signal of the state, on the UI thread; called once.
    pub(crate) fn signal(&self) -> Signal<State> {
        self.handover.signal(State::default())
    }

    fn hand_over(&self, inner: &Inner) {
        let live = inner
            .handle
            .as_ref()
            .map(Handle::notifications)
            .unwrap_or_default();
        self.handover.deliver(State {
            live: Arc::new(live),
            ended: inner.ended,
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A panic while holding the lock leaves a valid `Inner`.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

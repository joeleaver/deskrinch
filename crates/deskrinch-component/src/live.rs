//! One table of the configuration while the component runs.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use deskrinch_config::{Event, Section, Settings, Subscription, Watcher};
use rinch::prelude::Signal;

use crate::configuration::Shared;
use crate::report;

/// One table of the configuration, from [`Configuration::follow`]: read
/// when the component starts and followed while it runs.
///
/// [`initial`](Self::initial) is the table when the component started, for
/// making its window. [`into_signal`](Self::into_signal), in the root
/// component, hands the table to the UI: a `Signal<T>` that every later
/// change is sent to with [`Signal::send`], rinch's write from another
/// thread. Changes that came before are held until then. Diagnostics are
/// [reported](report::diagnostic) as they come; a broken file leaves the
/// last good settings in place (the configuration crate's rule).
///
/// It holds the process's configuration watcher, so the watcher runs for as
/// long as any table follows it.
///
/// [`Configuration::follow`]: crate::Configuration::follow
pub struct LiveSettings<T: Settings> {
    /// The table when the component started.
    pub initial: T,
    inbox: Arc<Inbox<T>>,
    // Declared before `shared`: deliveries stop before the watcher can go.
    subscription: Option<Subscription>,
    shared: Arc<Shared>,
}

impl<T: Settings> LiveSettings<T> {
    /// Subscribes `section` to `watcher`, which `shared` holds.
    pub(crate) fn subscribe(watcher: &Watcher, section: Section<T>, shared: Arc<Shared>) -> Self {
        let inbox = Arc::new(Inbox::default());
        let deliver = Arc::clone(&inbox);
        let (subscription, resolved) = watcher.subscribe(section, move |event| match event {
            Event::Changed(settings) => deliver.deliver(settings),
            Event::Diagnostics(diagnostics) => diagnostics.iter().for_each(report::diagnostic),
        });
        resolved.diagnostics.iter().for_each(report::diagnostic);
        Self {
            initial: resolved.settings,
            inbox,
            subscription: Some(subscription),
            shared,
        }
    }

    /// `initial`, not followed.
    pub(crate) fn fixed(initial: T, shared: Arc<Shared>) -> Self {
        Self {
            initial,
            inbox: Arc::default(),
            subscription: None,
            shared,
        }
    }

    /// Hands the table to the UI, in a component on the UI thread: a
    /// signal holding the newest settings, which every later change is
    /// written to. The table is followed until the component is disposed.
    pub fn into_signal(self) -> Signal<T> {
        let signal = Signal::new(self.initial.clone());
        if let Some(newer) = self.inbox.connect(signal) {
            signal.set(newer);
        }
        rinch::reactive::on_cleanup(move || drop(self));
        signal
    }
}

impl<T: Settings + fmt::Debug> fmt::Debug for LiveSettings<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LiveSettings")
            .field("initial", &self.initial)
            .field("following", &self.subscription.is_some())
            .field("watched", &self.shared.watcher.is_some())
            .finish_non_exhaustive()
    }
}

/// Where the watcher's thread hands the settings to the UI.
struct Inbox<T: 'static>(Mutex<Target<T>>);

enum Target<T: 'static> {
    /// No signal yet; the newest change so far.
    Waiting(Option<T>),
    /// The UI's signal.
    Connected(Signal<T>),
}

impl<T> Default for Inbox<T> {
    fn default() -> Self {
        Self(Mutex::new(Target::Waiting(None)))
    }
}

impl<T: Send> Inbox<T> {
    fn deliver(&self, settings: T) {
        // A panic while holding the lock leaves a valid `Target`.
        let mut target = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match &mut *target {
            Target::Waiting(pending) => *pending = Some(settings),
            Target::Connected(signal) => signal.send(settings),
        }
    }

    /// Sends every later change to `signal`, and returns the newest change
    /// that came before, if any.
    fn connect(&self, signal: Signal<T>) -> Option<T> {
        let mut target = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match std::mem::replace(&mut *target, Target::Connected(signal)) {
            Target::Waiting(pending) => pending,
            Target::Connected(_) => None,
        }
    }
}

#[cfg(test)]
impl<T: Settings> LiveSettings<T> {
    /// The change held for the UI, if any: for the tests, which have no
    /// UI to connect.
    pub(crate) fn pending(&self) -> Option<T> {
        match &*self.inbox.0.lock().unwrap_or_else(PoisonError::into_inner) {
            Target::Waiting(pending) => pending.clone(),
            Target::Connected(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskrinch_settings::panel::{Edge, PanelSettings};

    fn top() -> PanelSettings {
        PanelSettings {
            edge: Edge::Top,
            ..PanelSettings::default()
        }
    }

    #[test]
    fn holds_a_change_until_the_ui_connects_then_sends() {
        let inbox = Inbox::default();
        inbox.deliver(PanelSettings::default());
        inbox.deliver(top());
        let signal = Signal::new(PanelSettings::default());
        assert_eq!(inbox.connect(signal), Some(top()));
        inbox.deliver(PanelSettings::default());
        assert_eq!(signal.get(), PanelSettings::default());
        inbox.deliver(top());
        assert_eq!(signal.get(), top());
    }

    #[test]
    fn connecting_with_nothing_pending_gives_nothing() {
        let inbox = Inbox::<PanelSettings>::default();
        assert_eq!(inbox.connect(Signal::new(PanelSettings::default())), None);
    }
}

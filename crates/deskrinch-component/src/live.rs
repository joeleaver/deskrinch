//! One table of the configuration while the component runs.

use std::fmt;
use std::sync::Arc;

use deskrinch_config::{Event, Section, Settings, Subscription, Watcher};
use rinch::prelude::Signal;

use crate::configuration::Shared;
use crate::handover::Handover;
use crate::report;

/// One table of the configuration, from [`Configuration::follow`]: read
/// when the component starts and followed while it runs.
///
/// [`initial`](Self::initial) is the table when the component started, for
/// making its window. [`into_signal`](Self::into_signal), in the root
/// component, hands the table to the UI: a `Signal<T>` that every later
/// change is sent to with [`Signal::send`], rinch's write from another
/// thread. Changes that came before are held until then (a [`Handover`]). Diagnostics are
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
    inbox: Arc<Handover<T>>,
    // Declared before `shared`: deliveries stop before the watcher can go.
    subscription: Option<Subscription>,
    shared: Arc<Shared>,
}

impl<T: Settings> LiveSettings<T> {
    /// Subscribes `section` to `watcher`, which `shared` holds.
    pub(crate) fn subscribe(watcher: &Watcher, section: Section<T>, shared: Arc<Shared>) -> Self {
        let inbox = Arc::new(Handover::default());
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
        let signal = self.inbox.signal(self.initial.clone());
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

#[cfg(test)]
impl<T: Settings> LiveSettings<T> {
    /// The change held for the UI, if any: for the tests, which have no
    /// UI to connect.
    pub(crate) fn pending(&self) -> Option<T> {
        self.inbox.pending()
    }
}

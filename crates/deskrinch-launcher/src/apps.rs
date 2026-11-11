//! The installed applications, followed for the UI: the catalog scanned at
//! start and watched (`deskrinch-desktop-entries`' `Watcher`), the shown
//! entries handed to the UI at each change.

use std::sync::{Arc, Mutex, PoisonError};

use deskrinch_component::{Handover, report};
use deskrinch_desktop_entries::{Catalog, DesktopEntry, Environment, Watcher};
use deskrinch_launcher::Applications;
use rinch::prelude::Signal;

/// The shown applications, now and as they change.
pub(crate) struct Apps {
    initial: Arc<Vec<DesktopEntry>>,
    later: Arc<Handover<Arc<Vec<DesktopEntry>>>>,
    /// Kept while the launcher runs; `None` when watching could not start
    /// (reported), and the catalog scanned at start is all there is.
    watcher: Option<Watcher>,
}

impl Apps {
    /// Scans the applications of `env` and starts following them.
    pub(crate) fn start(env: &Environment) -> Self {
        let later: Arc<Handover<Arc<Vec<DesktopEntry>>>> = Arc::default();
        let state = Arc::new(Mutex::new(Applications::default()));
        let deliver = Arc::clone(&later);
        let follow = Arc::clone(&state);
        // Held while the watcher starts, so that a change it reports before
        // the scan is in place waits for it rather than being lost.
        let mut held = state.lock().unwrap_or_else(PoisonError::into_inner);
        let started = Watcher::start(env.clone(), move |event| {
            let mut applications = follow.lock().unwrap_or_else(PoisonError::into_inner);
            if let deskrinch_desktop_entries::Event::Error(error) = &event {
                report::error("following the installed applications", error);
            }
            if applications.apply(event) {
                deliver.deliver(applications.shown());
            }
        });
        let watcher = match started {
            Ok((watcher, catalog)) => {
                *held = Applications::new(&catalog);
                Some(watcher)
            }
            Err(error) => {
                report::error("following the installed applications", &error);
                *held = Applications::new(&Catalog::scan(env));
                None
            }
        };
        let initial = held.shown();
        drop(held);
        Self {
            initial,
            later,
            watcher,
        }
    }

    /// The UI's signal of the shown applications, on the UI thread; the
    /// watcher is kept until the component that calls this is disposed.
    pub(crate) fn into_signal(self) -> Signal<Arc<Vec<DesktopEntry>>> {
        let signal = self.later.signal(self.initial);
        let watcher = self.watcher;
        rinch::reactive::on_cleanup(move || drop(watcher));
        signal
    }
}

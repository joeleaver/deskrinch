//! [`Installed`]: the installed applications, followed for the UI: the
//! catalog scanned at start and watched (`deskrinch-desktop-entries`'
//! `Watcher`), and handed to the UI as a whole at each change.

use std::sync::{Arc, Mutex, PoisonError};

use deskrinch_component::{Handover, report};
use deskrinch_desktop_entries::{Catalog, Environment, Watcher};
use rinch::prelude::Signal;

use crate::Applications;

/// The installed applications, now and as they change.
pub struct Installed {
    initial: Arc<Applications>,
    /// Each change's whole catalog. A [`Handover`] keeps only the newest,
    /// which is all the UI needs of a whole catalog.
    later: Arc<Handover<Arc<Applications>>>,
    /// Kept while the component runs; `None` when watching could not start
    /// (reported), and the catalog scanned at start is all there is.
    watcher: Option<Watcher>,
}

impl Installed {
    /// Scans the applications of `env` and starts following them.
    #[must_use]
    pub fn start(env: &Environment) -> Self {
        let later: Arc<Handover<Arc<Applications>>> = Arc::default();
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
                deliver.deliver(Arc::new(applications.clone()));
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
        let initial = Arc::new(held.clone());
        drop(held);
        Self {
            initial,
            later,
            watcher,
        }
    }

    /// The UI's signal of the installed applications, on the UI thread; the
    /// watcher is kept until the component that calls this is disposed.
    #[must_use]
    pub fn into_signal(self) -> Signal<Arc<Applications>> {
        let signal = self.later.signal(self.initial);
        let watcher = self.watcher;
        rinch::reactive::on_cleanup(move || drop(watcher));
        signal
    }
}

impl std::fmt::Debug for Installed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Installed")
            .field("initial", &self.initial)
            .field("watching", &self.watcher.is_some())
            .finish_non_exhaustive()
    }
}

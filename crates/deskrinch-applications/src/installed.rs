//! [`Installed`]: the installed applications, followed for the UI: the
//! catalog scanned and watched (`deskrinch-desktop-entries`' `Watcher`) on a
//! thread of its own, and handed to the UI as a whole once scanned and at
//! each change.
//!
//! The scan is off the UI's path: a component draws its first frame without
//! waiting for every desktop file, and the applications fill in when the
//! scan answers. Until then the catalog is empty.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use deskrinch_component::{Handover, report};
use deskrinch_desktop_entries::{Catalog, Environment, Watcher};
use rinch::prelude::Signal;

use crate::Applications;

/// The watcher, as the thread that starts it and the UI that ends it share
/// it.
enum Keep {
    /// The scan has not answered yet.
    Starting,
    /// Kept while the component runs; `None` when watching could not start
    /// (reported), and the catalog scanned is all there is.
    Running(Option<Watcher>),
    /// The component is gone: a watcher that starts now is dropped.
    Ended,
}

/// The installed applications, empty until scanned, and as they change.
pub struct Installed {
    /// Each change's whole catalog, the scan's first. A [`Handover`] keeps
    /// only the newest, which is all the UI needs of a whole catalog.
    later: Arc<Handover<Arc<Applications>>>,
    keep: Arc<Mutex<Keep>>,
}

impl Installed {
    /// Starts scanning the applications of `env` and following them, on a
    /// thread of its own; returns at once.
    #[must_use]
    pub fn start(env: &Environment) -> Self {
        let later: Arc<Handover<Arc<Applications>>> = Arc::default();
        let keep = Arc::new(Mutex::new(Keep::Starting));
        let (deliver, kept, env) = (Arc::clone(&later), Arc::clone(&keep), env.clone());
        let spawned = std::thread::Builder::new()
            .name("applications".into())
            .spawn(move || {
                let watcher = scan_and_follow(&env, &deliver);
                let mut keep = lock(&kept);
                if matches!(*keep, Keep::Starting) {
                    *keep = Keep::Running(watcher);
                }
                // Ended meanwhile: the watcher is dropped here.
            });
        if let Err(error) = spawned {
            report::error("starting the thread that follows the applications", &error);
        }
        Self { later, keep }
    }

    /// The UI's signal of the installed applications, on the UI thread:
    /// empty until the scan answers. The watcher is kept until the
    /// component that calls this is disposed.
    #[must_use]
    pub fn into_signal(self) -> Signal<Arc<Applications>> {
        let signal = self.later.signal(Arc::default());
        let keep = self.keep;
        rinch::reactive::on_cleanup(move || {
            let ended = std::mem::replace(&mut *lock(&keep), Keep::Ended);
            drop(ended);
        });
        signal
    }
}

/// Scans `env`'s applications and follows them, handing `deliver` the
/// scanned catalog and then each change's; the watcher, when it started.
fn scan_and_follow(
    env: &Environment,
    deliver: &Arc<Handover<Arc<Applications>>>,
) -> Option<Watcher> {
    let state = Arc::new(Mutex::new(Applications::default()));
    let changes = Arc::clone(deliver);
    let follow = Arc::clone(&state);
    // Held while the watcher starts, so that a change it reports before the
    // scan is in place waits for it rather than being lost.
    let mut held = lock(&state);
    let started = Watcher::start(env.clone(), move |event| {
        let mut applications = lock(&follow);
        if let deskrinch_desktop_entries::Event::Error(error) = &event {
            report::error("following the installed applications", error);
        }
        if applications.apply(event) {
            changes.deliver(Arc::new(applications.clone()));
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
    deliver.deliver(Arc::new(held.clone()));
    watcher
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl std::fmt::Debug for Installed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = match &*lock(&self.keep) {
            Keep::Starting => "starting",
            Keep::Running(Some(_)) => "watching",
            Keep::Running(None) => "scanned, not watching",
            Keep::Ended => "ended",
        };
        f.debug_struct("Installed")
            .field("state", &state)
            .finish_non_exhaustive()
    }
}

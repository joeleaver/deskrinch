//! Watching the data directories for entries added, changed or removed.
//!
//! Every `applications` directory is watched as a tree through
//! `deskrinch-watch`: recursively once it exists, through its nearest
//! existing ancestor until then (so its creation is noticed), and where a
//! symbolic link to it points. After a burst of changes settles the
//! watcher rescans everything, diffs the new catalog against the last one
//! and reports the difference: rescanning applies precedence, `Hidden` and
//! the visibility rules exactly as [`Catalog::scan`] does, at the cost of
//! reading a few hundred small files.
//!
//! Only the data directories are watched, not `PATH`. An entry's visibility
//! also depends on whether its `TryExec` program exists; that is
//! re-evaluated on every rescan, and [`Watcher::refresh`] asks for one, so
//! a launcher calls it when it opens to show nothing stale.

use std::collections::BTreeSet;
use std::sync::Arc;

use deskrinch_watch::{Armed, Target, Timing, WatchError};

use crate::catalog::{Catalog, CatalogEntry};
use crate::entry::DesktopId;
use crate::env::Environment;
use crate::scan::application_dirs;

/// A change to the catalog.
#[derive(Debug)]
pub enum Event {
    /// An entry appeared: a new ID, or one a removed `Hidden` entry
    /// uncovered. The catalog's own, shared.
    Added(Arc<CatalogEntry>),
    /// An entry changed: its file, or which file wins for its ID, or its
    /// visibility. The new entry, shared with the catalog.
    Changed(Arc<CatalogEntry>),
    /// The entry with this ID is gone (deleted, or hidden by `Hidden=true`).
    Removed(DesktopId),
    /// Watching failed in part; the watcher carries on and rescans.
    Error(WatchError),
}

/// Watches the data directories and reports [`Event`]s to a subscriber
/// until dropped.
#[derive(Debug)]
pub struct Watcher {
    watch: deskrinch_watch::Watcher,
}

impl Watcher {
    /// Starts watching the data directories of `env`, and returns the
    /// catalog the reported events are changes to. `subscriber` is called
    /// on the watcher's thread, one event at a time.
    ///
    /// # Errors
    ///
    /// [`WatchError`] when the file-watching backend or the thread cannot
    /// be started, or a directory cannot be watched (for example
    /// [`WatchError::WatchLimit`], when the inotify watch limit is reached).
    pub fn start(
        env: Environment,
        subscriber: impl Fn(Event) + Send + 'static,
    ) -> Result<(Self, Catalog), WatchError> {
        let targets = application_dirs(&env)
            .into_iter()
            .map(Target::Tree)
            .collect();
        // Watch first, then scan: a change between the two is reported
        // rather than lost.
        let armed = Armed::new(targets, Timing::default())?;
        let catalog = Catalog::scan(&env);
        let mut last = catalog.clone();
        let watch = armed.start("desktop-entries-watch", move |event| match event {
            deskrinch_watch::Event::Changed => {
                // Entries that did not change keep the allocation everything
                // already shares.
                let newer = Catalog::scan(&env).reusing(&last);
                for event in diff(&last, &newer) {
                    subscriber(event);
                }
                last = newer;
            }
            deskrinch_watch::Event::Error(error) => subscriber(Event::Error(error)),
        })?;
        Ok((Self { watch }, catalog))
    }

    /// Rescans now, re-evaluating every entry's visibility, and reports the
    /// changes through the subscriber like any others.
    ///
    /// `PATH` is not watched, so installing or removing a `TryExec` program
    /// changes no file the watcher sees. Call this when that may matter,
    /// such as when the launcher opens. It returns at once; the events
    /// follow on the watcher's thread.
    pub fn refresh(&self) {
        self.watch.refresh();
    }
}

/// The events that turn `older` into `newer`, in ID order.
pub(crate) fn diff(older: &Catalog, newer: &Catalog) -> Vec<Event> {
    let (older, newer) = (older.entries(), newer.entries());
    let ids: BTreeSet<&DesktopId> = older.keys().chain(newer.keys()).collect();
    ids.into_iter()
        .filter_map(|id| match (older.get(id), newer.get(id)) {
            (None, Some(added)) => Some(Event::Added(Arc::clone(added))),
            (Some(_), None) => Some(Event::Removed(id.clone())),
            (Some(old), Some(new)) if old != new => Some(Event::Changed(Arc::clone(new))),
            _ => None,
        })
        .collect()
}

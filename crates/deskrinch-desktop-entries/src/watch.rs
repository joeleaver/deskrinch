//! Watching the data directories for entries added, changed or removed.
//!
//! The watcher runs on a thread of its own. It watches every `applications`
//! directory recursively and, for one that does not exist yet, the nearest
//! existing parent (so its creation is noticed). After a burst of changes
//! settles it rescans everything, diffs the new catalog against the last
//! one and reports the difference: rescanning applies precedence, `Hidden`
//! and the visibility rules exactly as [`Catalog::scan`] does, at the cost
//! of reading a few hundred small files.
//!
//! Only the files are watched: an entry's visibility also depends on
//! whether its `TryExec` program exists, which is re-evaluated whenever a
//! rescan happens but does not by itself cause one.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::catalog::{Catalog, CatalogEntry};
use crate::entry::DesktopId;
use crate::env::Environment;
use crate::error::WatchError;
use crate::scan::application_dirs;

/// How long changes must pause before a rescan.
const QUIET: Duration = Duration::from_millis(100);
/// The longest a rescan waits during a continuous stream of changes.
const MAX_DELAY: Duration = Duration::from_secs(1);

/// A change to the catalog.
#[derive(Debug)]
pub enum Event {
    /// An entry appeared: a new ID, or one a removed `Hidden` entry
    /// uncovered.
    Added(CatalogEntry),
    /// An entry changed: its file, or which file wins for its ID, or its
    /// visibility.
    Changed(CatalogEntry),
    /// The entry with this ID is gone (deleted, or hidden by `Hidden=true`).
    Removed(DesktopId),
    /// Watching failed in part; the watcher carries on and rescans.
    Error(WatchError),
}

/// Watches the data directories and reports [`Event`]s to a subscriber
/// until dropped.
#[derive(Debug)]
pub struct Watcher {
    messages: Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

enum Message {
    Changed(notify::Result<notify::Event>),
    Stop,
}

impl Watcher {
    /// Starts watching the data directories of `env`, and returns the
    /// catalog the reported events are changes to. `subscriber` is called
    /// on the watcher's thread, one event at a time.
    ///
    /// # Errors
    ///
    /// [`WatchError`] when the file-watching backend or the thread cannot
    /// be started, or a directory cannot be watched (for example when the
    /// inotify watch limit is reached).
    pub fn start(
        env: Environment,
        subscriber: impl Fn(Event) + Send + 'static,
    ) -> Result<(Self, Catalog), WatchError> {
        let (messages, inbox) = mpsc::channel();
        let forward = messages.clone();
        let backend = notify::recommended_watcher(move |change| {
            // A send fails only once the worker has stopped, when there is
            // no one left to tell.
            forward.send(Message::Changed(change)).ok();
        })
        .map_err(|error| WatchError::Backend(Box::new(error)))?;
        let mut worker = Worker {
            backend,
            targets: Targets::default(),
            env,
            catalog: Catalog::default(),
            subscriber: Box::new(subscriber),
        };
        // Watch first, then scan: a change between the two is reported
        // rather than lost.
        if let Some(error) = worker.retarget().into_iter().next() {
            return Err(error);
        }
        worker.catalog = Catalog::scan(&worker.env);
        let catalog = worker.catalog.clone();
        let handle = thread::Builder::new()
            .name("desktop-entries-watch".into())
            .spawn(move || worker.run(&inbox))
            .map_err(WatchError::Thread)?;
        Ok((
            Self {
                messages,
                worker: Some(handle),
            },
            catalog,
        ))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.messages.send(Message::Stop).ok();
        if let Some(worker) = self.worker.take() {
            // A panic in the subscriber ended the thread already; there is
            // nothing more to stop.
            worker.join().ok();
        }
    }
}

struct Worker {
    backend: RecommendedWatcher,
    targets: Targets,
    env: Environment,
    catalog: Catalog,
    subscriber: Box<dyn Fn(Event) + Send>,
}

impl Worker {
    fn run(mut self, inbox: &Receiver<Message>) {
        while let Ok(Message::Changed(change)) = inbox.recv() {
            let mut relevant = self.consider(change);
            let deadline = Instant::now() + MAX_DELAY;
            loop {
                let wait = QUIET.min(deadline.saturating_duration_since(Instant::now()));
                match inbox.recv_timeout(wait) {
                    Ok(Message::Changed(change)) => relevant |= self.consider(change),
                    Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => break,
                }
            }
            if relevant {
                for error in self.retarget() {
                    (self.subscriber)(Event::Error(error));
                }
                self.rescan();
            }
        }
    }

    /// Whether a change needs a rescan. Reading files (as the scan itself
    /// does) changes nothing; nor does a change beside a missing
    /// `applications` directory's path.
    fn consider(&self, change: notify::Result<notify::Event>) -> bool {
        match change {
            Ok(event) => {
                !matches!(event.kind, EventKind::Access(_))
                    && event.paths.iter().any(|path| self.targets.concerns(path))
            }
            Err(error) => {
                (self.subscriber)(Event::Error(WatchError::Backend(Box::new(error))));
                true
            }
        }
    }

    /// Watches what the directories now need, and returns the watches that
    /// could not be added.
    fn retarget(&mut self) -> Vec<WatchError> {
        let wanted = Targets::for_dirs(&application_dirs(&self.env));
        for (path, _) in self.targets.watched.difference(&wanted.watched) {
            // The path may be gone already, which removed its watch.
            self.backend.unwatch(path).ok();
        }
        let mut errors = Vec::new();
        for (path, recursive) in wanted.watched.difference(&self.targets.watched) {
            let mode = if *recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            if let Err(error) = self.backend.watch(path, mode) {
                errors.push(WatchError::Backend(Box::new(error)));
            }
        }
        self.targets = wanted;
        errors
    }

    fn rescan(&mut self) {
        let newer = Catalog::scan(&self.env);
        for event in diff(&self.catalog, &newer) {
            (self.subscriber)(event);
        }
        self.catalog = newer;
    }
}

/// The paths watched: `(path, recursive)`.
#[derive(Debug, Default, PartialEq, Eq)]
struct Targets {
    watched: BTreeSet<(PathBuf, bool)>,
    /// The `applications` directories, existing or not.
    dirs: Vec<PathBuf>,
}

impl Targets {
    fn for_dirs(dirs: &[PathBuf]) -> Self {
        let watched = dirs
            .iter()
            .filter_map(|dir| {
                if dir.is_dir() {
                    return Some((dir.clone(), true));
                }
                let parent = dir.ancestors().skip(1).find(|a| a.is_dir())?;
                Some((parent.to_path_buf(), false))
            })
            .collect();
        Self {
            watched,
            dirs: dirs.to_vec(),
        }
    }

    /// Whether a change at `path` may affect an `applications` directory:
    /// it is inside one, or on the way to one.
    fn concerns(&self, path: &Path) -> bool {
        self.dirs
            .iter()
            .any(|dir| path.starts_with(dir) || dir.starts_with(path))
    }
}

/// The events that turn `older` into `newer`, in ID order.
pub(crate) fn diff(older: &Catalog, newer: &Catalog) -> Vec<Event> {
    let (older, newer) = (older.entries(), newer.entries());
    let ids: BTreeSet<&DesktopId> = older.keys().chain(newer.keys()).collect();
    ids.into_iter()
        .filter_map(|id| match (older.get(id), newer.get(id)) {
            (None, Some(added)) => Some(Event::Added(added.clone())),
            (Some(_), None) => Some(Event::Removed(id.clone())),
            (Some(old), Some(new)) if old != new => Some(Event::Changed(new.clone())),
            _ => None,
        })
        .collect()
}

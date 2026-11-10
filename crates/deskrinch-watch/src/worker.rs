//! The watching thread: changes in, debounced, filtered and reported.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use notify::{EventKind, RecursiveMode};

use crate::error::WatchError;
use crate::plan::Plan;
use crate::{Event, Target, Timing};

/// How many times a retarget re-plans when a directory vanishes between
/// planning a watch and adding it.
const REPLANS: usize = 3;

/// What adds and removes watches: notify's watcher, or a stand-in in tests.
pub(crate) trait Backend: Send {
    fn watch(&mut self, dir: &Path, recursive: bool) -> notify::Result<()>;
    fn unwatch(&mut self, dir: &Path) -> notify::Result<()>;
}

impl Backend for notify::RecommendedWatcher {
    fn watch(&mut self, dir: &Path, recursive: bool) -> notify::Result<()> {
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        notify::Watcher::watch(self, dir, mode)
    }

    fn unwatch(&mut self, dir: &Path) -> notify::Result<()> {
        notify::Watcher::unwatch(self, dir)
    }
}

pub(crate) enum Message {
    Changed(notify::Result<notify::Event>),
    Refresh,
    Stop,
}

/// Targets being watched, with no thread reporting yet: changes queue
/// until [`start`](Self::start).
///
/// Arming, then reading what the targets hold, then starting, is how a
/// caller misses nothing: a change after arming is reported, even one that
/// happens while it reads.
pub struct Armed {
    core: Core,
    messages: Sender<Message>,
    inbox: Receiver<Message>,
}

impl std::fmt::Debug for Armed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Armed")
            .field("targets", &self.core.targets)
            .field("plan", &self.core.plan)
            .finish_non_exhaustive()
    }
}

struct Core {
    backend: Box<dyn Backend>,
    targets: Vec<Target>,
    timing: Timing,
    plan: Plan,
}

impl Armed {
    /// Starts watching `targets`.
    ///
    /// # Errors
    ///
    /// [`WatchError`] when the backend cannot be created, or a directory
    /// cannot be watched (for example when the inotify watch limit is
    /// reached).
    pub fn new(targets: Vec<Target>, timing: Timing) -> Result<Self, WatchError> {
        let (messages, inbox) = mpsc::channel();
        let forward = messages.clone();
        let backend = notify::recommended_watcher(move |change| {
            // A send fails only once the worker has stopped, when there is
            // no one left to tell.
            forward.send(Message::Changed(change)).ok();
        })
        .map_err(WatchError::from_backend)?;
        Self::with_backend(Box::new(backend), messages, inbox, targets, timing)
    }

    pub(crate) fn with_backend(
        backend: Box<dyn Backend>,
        messages: Sender<Message>,
        inbox: Receiver<Message>,
        targets: Vec<Target>,
        timing: Timing,
    ) -> Result<Self, WatchError> {
        let mut core = Core {
            backend,
            targets,
            timing,
            plan: Plan::default(),
        };
        if let Some(error) = core.retarget().into_iter().next() {
            return Err(error);
        }
        Ok(Self {
            core,
            messages,
            inbox,
        })
    }

    /// Starts the thread, named `name`, that calls `subscriber` with the
    /// [`Event`]s, one at a time, until the returned [`Watcher`] is
    /// dropped.
    ///
    /// # Errors
    ///
    /// [`WatchError::Thread`] when the thread cannot be started.
    pub fn start(
        self,
        name: &str,
        subscriber: impl FnMut(Event) + Send + 'static,
    ) -> Result<Watcher, WatchError> {
        let Self {
            core,
            messages,
            inbox,
        } = self;
        let worker = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || core.run(&inbox, subscriber))
            .map_err(WatchError::Thread)?;
        Ok(Watcher {
            messages,
            worker: Some(worker),
        })
    }
}

/// Watches targets and reports [`Event`]s to a subscriber, on a thread of
/// its own, until dropped.
#[derive(Debug)]
pub struct Watcher {
    messages: Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Arms and starts at once, for a caller with nothing to read between
    /// the two; see [`Armed`].
    ///
    /// # Errors
    ///
    /// As [`Armed::new`] and [`Armed::start`].
    pub fn start(
        targets: Vec<Target>,
        timing: Timing,
        name: &str,
        subscriber: impl FnMut(Event) + Send + 'static,
    ) -> Result<Self, WatchError> {
        Armed::new(targets, timing)?.start(name, subscriber)
    }

    /// Reports [`Event::Changed`] now, as if a target had changed, without
    /// waiting for a pause: for what a caller depends on that no file it
    /// watches shows (a program installed on `PATH`). Returns at once; the
    /// event follows on the watcher's thread. During a burst of changes it
    /// joins the burst's one report.
    pub fn refresh(&self) {
        // Fails only once the worker has stopped.
        self.messages.send(Message::Refresh).ok();
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.messages.send(Message::Stop).ok();
        if let Some(worker) = self.worker.take()
            // Dropped by its own subscriber: the thread ends when the
            // subscriber returns, and cannot wait for itself.
            && worker.thread().id() != thread::current().id()
        {
            // A panic in the subscriber ended the thread already; there is
            // nothing more to stop.
            worker.join().ok();
        }
    }
}

impl Core {
    fn run(mut self, inbox: &Receiver<Message>, mut subscriber: impl FnMut(Event)) {
        loop {
            let relevant = match inbox.recv() {
                // Asked for now: no waiting for a pause.
                Ok(Message::Refresh) => true,
                Ok(Message::Changed(change)) => {
                    let mut relevant = self.consider(change, &mut subscriber);
                    let deadline = Instant::now() + self.timing.max_delay;
                    loop {
                        let wait = self
                            .timing
                            .quiet
                            .min(deadline.saturating_duration_since(Instant::now()));
                        match inbox.recv_timeout(wait) {
                            Ok(Message::Changed(change)) => {
                                relevant |= self.consider(change, &mut subscriber);
                            }
                            Ok(Message::Refresh) => relevant = true,
                            Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                            Err(RecvTimeoutError::Timeout) => break,
                        }
                    }
                    relevant
                }
                Ok(Message::Stop) | Err(_) => return,
            };
            if relevant {
                for error in self.retarget() {
                    subscriber(Event::Error(error));
                }
                subscriber(Event::Changed);
            }
        }
    }

    /// Whether a change may concern a target. Reading files changes
    /// nothing. A backend error is reported, and counts as a change: what
    /// it missed is unknown.
    fn consider(
        &self,
        change: notify::Result<notify::Event>,
        subscriber: &mut impl FnMut(Event),
    ) -> bool {
        match change {
            Ok(event) => {
                !matches!(event.kind, EventKind::Access(_))
                    && event.paths.iter().any(|path| self.plan.concerns(path))
            }
            Err(error) => {
                subscriber(Event::Error(WatchError::from_backend(error)));
                true
            }
        }
    }

    /// Watches what the targets now need, and returns the watches that
    /// could not be added.
    fn retarget(&mut self) -> Vec<WatchError> {
        let mut errors = Vec::new();
        for _ in 0..REPLANS {
            errors = self.apply(Plan::new(&self.targets));
            if !errors.iter().any(WatchError::is_path_not_found) {
                break;
            }
        }
        errors
    }

    fn apply(&mut self, wanted: Plan) -> Vec<WatchError> {
        for (dir, recursive) in &self.plan.watches {
            if wanted.watches.get(dir) != Some(recursive) {
                // The directory may be gone already, which removed its
                // watch.
                self.backend.unwatch(dir).ok();
            }
        }
        let mut errors = Vec::new();
        for (dir, recursive) in &wanted.watches {
            if self.plan.watches.get(dir) != Some(recursive)
                && let Err(error) = self.backend.watch(dir, *recursive)
            {
                errors.push(WatchError::from_backend(error.add_path(dir.clone())));
            }
        }
        self.plan = wanted;
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A backend that records watches and refuses any beyond `limit`, as
    /// inotify does at `max_user_watches`.
    struct Limited {
        limit: usize,
        watched: Arc<Mutex<Vec<std::path::PathBuf>>>,
    }

    impl Backend for Limited {
        fn watch(&mut self, dir: &Path, _recursive: bool) -> notify::Result<()> {
            let mut watched = self.watched.lock().unwrap();
            if watched.len() >= self.limit {
                return Err(notify::Error::new(notify::ErrorKind::MaxFilesWatch));
            }
            watched.push(dir.to_path_buf());
            Ok(())
        }

        fn unwatch(&mut self, dir: &Path) -> notify::Result<()> {
            self.watched.lock().unwrap().retain(|d| d != dir);
            Ok(())
        }
    }

    fn arm(limit: usize, targets: Vec<Target>) -> (Result<Armed, WatchError>, Sender<Message>) {
        let (messages, inbox) = mpsc::channel();
        let backend = Limited {
            limit,
            watched: Arc::default(),
        };
        let armed = Armed::with_backend(
            Box::new(backend),
            messages.clone(),
            inbox,
            targets,
            Timing::default(),
        );
        (armed, messages)
    }

    #[test]
    fn reports_the_watch_limit_when_arming() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("a")).unwrap();
        let targets = vec![Target::File(root.join("a/f")), Target::File(root.join("g"))];
        let (armed, _) = arm(1, targets);
        let error = armed.unwrap_err();
        assert!(
            matches!(&error, WatchError::WatchLimit { path: Some(p) } if p == &root.join("a")),
            "{error:?}"
        );
    }

    #[test]
    fn reports_the_watch_limit_when_the_backend_hits_it_later() {
        let root = tempfile::tempdir().unwrap();
        let (armed, messages) = arm(8, vec![Target::Tree(root.path().to_path_buf())]);
        let (events, received) = mpsc::channel();
        let watcher = armed
            .unwrap()
            .start("test-watch", move |event| {
                events.send(event).ok();
            })
            .unwrap();
        // What inotify reports when a directory created inside a recursive
        // watch cannot be watched.
        messages
            .send(Message::Changed(Err(notify::Error::new(
                notify::ErrorKind::MaxFilesWatch,
            ))))
            .unwrap();
        let timeout = std::time::Duration::from_secs(10);
        assert!(matches!(
            received.recv_timeout(timeout).unwrap(),
            Event::Error(WatchError::WatchLimit { .. })
        ));
        assert!(matches!(
            received.recv_timeout(timeout).unwrap(),
            Event::Changed
        ));
        drop(watcher);
    }
}

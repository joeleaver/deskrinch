//! The watching thread: changes in, debounced, filtered and reported.
//!
//! The thread waits in the kernel ([`Backend::wait`]) for a change or for a
//! message from a [`Handle`] or the [`Watcher`], which is sent on a channel
//! and announced by the backend's [`Waker`]. Nothing else wakes it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::error::WatchError;
use crate::inotify::Inotify;
use crate::plan::Plan;
use crate::{Event, Target, Timing};

/// How many times a retarget re-plans when a directory vanishes between
/// planning a watch and adding it.
const REPLANS: usize = 3;

/// How a directory is watched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct How {
    /// With every directory below it.
    pub(crate) recursive: bool,
    /// For its entries written or changed, not only made, removed and
    /// renamed.
    pub(crate) entries: bool,
}

/// What changed, as the backend read it.
#[derive(Debug)]
pub(crate) enum Change {
    /// Something at this path: an entry of a watched directory, or the
    /// directory itself.
    At(PathBuf),
    /// Something, where is not known: the kernel's queue overflowed, or it
    /// dropped a watch.
    Unknown,
    /// Watching failed in part (a directory made in a tree could not be
    /// watched); what it missed is unknown.
    Error(WatchError),
}

/// What one wait saw.
#[derive(Debug)]
pub(crate) enum Wait {
    /// Changes were read.
    Changes(Vec<Change>),
    /// The [`Waker`] was called: a message is waiting.
    Woken,
    /// The time ran out, or nothing that is a change arrived.
    Quiet,
    /// The backend cannot go on.
    Failed(WatchError),
}

/// Wakes the backend's wait, from any thread.
pub(crate) type Waker = Arc<dyn Fn() + Send + Sync>;

/// What adds and removes watches and waits for changes: inotify, or a
/// stand-in in tests.
pub(crate) trait Backend: Send {
    fn watch(&mut self, dir: &Path, how: How) -> Result<(), WatchError>;
    fn unwatch(&mut self, dir: &Path);
    /// Waits for changes or the waker, for at most `timeout` (none: as
    /// long as it takes).
    fn wait(&mut self, timeout: Option<Duration>) -> Wait;
    fn waker(&self) -> Waker;
}

pub(crate) enum Message {
    Refresh,
    Targets(Vec<Target>),
    Stop,
}

/// Sends messages to the watcher's thread, and wakes it to read them.
#[derive(Clone)]
struct Mailbox {
    messages: Sender<Message>,
    wake: Waker,
}

impl Mailbox {
    fn send(&self, message: Message) {
        // Fails only once the worker has stopped, when there is no one left
        // to tell.
        if self.messages.send(message).is_ok() {
            (self.wake)();
        }
    }
}

impl std::fmt::Debug for Mailbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mailbox").finish_non_exhaustive()
    }
}

/// Asks a watcher, from any thread, to report a change now or to watch
/// other targets. Cloning it is cheap. A handle keeps no watcher alive:
/// once the [`Watcher`] is dropped, what a handle asks is ignored.
#[derive(Debug, Clone)]
pub struct Handle {
    mailbox: Mailbox,
}

impl Handle {
    /// Reports [`Event::Changed`] now, as if a target had changed, without
    /// waiting for a pause: for what a caller depends on that no file it
    /// watches shows (a program installed on `PATH`). Returns at once; the
    /// event follows on the watcher's thread. During a burst of changes it
    /// joins the burst's one report.
    pub fn refresh(&self) {
        self.mailbox.send(Message::Refresh);
    }

    /// Watches `targets` instead of the targets so far, then reports
    /// [`Event::Changed`] as [`refresh`](Self::refresh) does, so the
    /// subscriber reads again what the new targets hold. For a caller
    /// whose targets depend on what it read (an icon theme's parents).
    pub fn set_targets(&self, targets: Vec<Target>) {
        self.mailbox.send(Message::Targets(targets));
    }
}

/// Targets being watched, with no thread reporting yet: changes queue
/// until [`start`](Self::start).
///
/// Arming, then reading what the targets hold, then starting, is how a
/// caller misses nothing: a change after arming is reported, even one that
/// happens while it reads.
pub struct Armed {
    core: Core,
    mailbox: Mailbox,
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
        Self::with_backend(Box::new(Inotify::new()?), targets, timing)
    }

    pub(crate) fn with_backend(
        backend: Box<dyn Backend>,
        targets: Vec<Target>,
        timing: Timing,
    ) -> Result<Self, WatchError> {
        let (messages, inbox) = mpsc::channel();
        let mailbox = Mailbox {
            messages,
            wake: backend.waker(),
        };
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
            mailbox,
            inbox,
        })
    }

    /// A handle to the watcher this becomes, for a subscriber that needs
    /// one before [`start`](Self::start) returns.
    #[must_use]
    pub fn handle(&self) -> Handle {
        Handle {
            mailbox: self.mailbox.clone(),
        }
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
            mailbox,
            inbox,
        } = self;
        let worker = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || core.run(&inbox, subscriber))
            .map_err(WatchError::Thread)?;
        Ok(Watcher {
            mailbox,
            worker: Some(worker),
        })
    }
}

/// Watches targets and reports [`Event`]s to a subscriber, on a thread of
/// its own, until dropped.
#[derive(Debug)]
pub struct Watcher {
    mailbox: Mailbox,
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

    /// A handle to this watcher; see [`Handle::refresh`].
    #[must_use]
    pub fn handle(&self) -> Handle {
        Handle {
            mailbox: self.mailbox.clone(),
        }
    }

    /// [`Handle::refresh`], on this watcher.
    pub fn refresh(&self) {
        self.handle().refresh();
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.mailbox.send(Message::Stop);
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

/// What one wait for the next thing heard gave.
enum Heard {
    /// Asked to report now (a refresh, new targets).
    Asked,
    /// Changes read; whether any concerns a target.
    Changes(bool),
    /// The time ran out.
    Nothing,
    /// Stop: asked to, or the backend failed.
    Stop,
}

impl Core {
    fn run(mut self, inbox: &Receiver<Message>, mut subscriber: impl FnMut(Event)) {
        loop {
            let relevant = match self.hear(inbox, None, &mut subscriber) {
                Heard::Stop => return,
                Heard::Nothing => continue,
                // Asked for now: no waiting for a pause.
                Heard::Asked => true,
                Heard::Changes(concerns) => {
                    let mut relevant = concerns;
                    let deadline = Instant::now() + self.timing.max_delay;
                    loop {
                        let wait = self
                            .timing
                            .quiet
                            .min(deadline.saturating_duration_since(Instant::now()));
                        match self.hear(inbox, Some(wait), &mut subscriber) {
                            Heard::Changes(concerns) => relevant |= concerns,
                            Heard::Asked => relevant = true,
                            Heard::Nothing => break,
                            Heard::Stop => return,
                        }
                    }
                    relevant
                }
            };
            if relevant {
                for error in self.retarget() {
                    subscriber(Event::Error(error));
                }
                subscriber(Event::Changed);
            }
        }
    }

    /// The next thing heard: a message first, else what the backend reads
    /// within `timeout`.
    fn hear(
        &mut self,
        inbox: &Receiver<Message>,
        timeout: Option<Duration>,
        subscriber: &mut impl FnMut(Event),
    ) -> Heard {
        let deadline = timeout.map(|timeout| Instant::now() + timeout);
        loop {
            match inbox.try_recv() {
                Ok(Message::Refresh) => return Heard::Asked,
                Ok(Message::Targets(targets)) => {
                    self.targets = targets;
                    return Heard::Asked;
                }
                Ok(Message::Stop) | Err(TryRecvError::Disconnected) => return Heard::Stop,
                Err(TryRecvError::Empty) => {}
            }
            let left = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
            match self.backend.wait(left) {
                Wait::Changes(changes) => {
                    return Heard::Changes(self.consider(changes, subscriber));
                }
                Wait::Woken => {}
                Wait::Quiet => {
                    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        return Heard::Nothing;
                    }
                }
                Wait::Failed(error) => {
                    subscriber(Event::Error(error));
                    return Heard::Stop;
                }
            }
        }
    }

    /// Whether `changes` may concern a target. A backend error is reported,
    /// and counts as a change: what it missed is unknown.
    fn consider(&self, changes: Vec<Change>, subscriber: &mut impl FnMut(Event)) -> bool {
        let mut relevant = false;
        for change in changes {
            relevant |= match change {
                Change::At(path) => self.plan.concerns(&path),
                Change::Unknown => true,
                Change::Error(error) => {
                    subscriber(Event::Error(error));
                    true
                }
            };
        }
        relevant
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
        let how = |plan: &Plan, dir: &Path, recursive: bool| How {
            recursive,
            entries: plan.wants_entries(dir),
        };
        for (dir, recursive) in &self.plan.watches {
            let was = how(&self.plan, dir, *recursive);
            let now = wanted.watches.get(dir).map(|r| how(&wanted, dir, *r));
            if now != Some(was) {
                self.backend.unwatch(dir);
            }
        }
        let mut errors = Vec::new();
        for (dir, recursive) in &wanted.watches {
            let now = how(&wanted, dir, *recursive);
            let was = self.plan.watches.get(dir).map(|r| how(&self.plan, dir, *r));
            if was != Some(now)
                && let Err(error) = self.backend.watch(dir, now)
            {
                errors.push(error);
            }
        }
        self.plan = wanted;
        errors
    }
}

#[cfg(test)]
mod tests;

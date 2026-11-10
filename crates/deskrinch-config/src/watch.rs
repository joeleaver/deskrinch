//! One watcher per process, delivering each subscribed section's new
//! settings.
//!
//! The [`Watcher`] watches every layer's file through `deskrinch-watch`
//! (so creation, removal, saves by rename and symbolic links are seen).
//! When a change settles it reads the files once and resolves every
//! subscribed section from that one snapshot; each subscriber hears only
//! what changed for its section.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use deskrinch_watch::{Armed, Target, Timing, WatchError};

use crate::diagnostic::Diagnostic;
use crate::section::{Resolved, Section, Settings};
use crate::snapshot::Snapshot;
use crate::sources::Sources;

/// What a reload found for one section.
#[derive(Debug)]
pub enum Event<T> {
    /// The settings changed; these are the new ones.
    Changed(T),
    /// The diagnostics changed (a file broke, was fixed, or gained or lost
    /// an unknown key); these are all of them now, and empty when nothing
    /// is wrong any more. Delivered before the [`Changed`](Self::Changed)
    /// of the same reload.
    Diagnostics(Vec<Diagnostic>),
}

/// Watches the configuration files for the whole process, and delivers
/// the [`Event`]s of every section [subscribed](Self::subscribe), until
/// dropped.
pub struct Watcher {
    // Declared first, so the thread stops before the rest is dropped.
    _watch: deskrinch_watch::Watcher,
    shared: Arc<Shared>,
}

/// A section's subscription; dropping it ends the deliveries.
pub struct Subscription {
    shared: Weak<Shared>,
    id: u64,
    active: Arc<AtomicBool>,
}

struct Shared {
    sources: Sources,
    subscribers: Mutex<Subscribers>,
}

#[derive(Default)]
struct Subscribers {
    next_id: u64,
    list: Vec<(u64, Box<dyn Reload>)>,
}

/// A deferred call of one subscriber with the events of one reload.
type Delivery = Box<dyn FnOnce() + Send>;

/// One section's state, without its type.
trait Reload: Send {
    /// Resolves the section from `snapshot`, and what to deliver, if
    /// anything changed.
    fn reload(&mut self, snapshot: &Snapshot) -> Option<Delivery>;
}

struct Typed<T> {
    section: Section<T>,
    /// The settings and diagnostics last delivered.
    settings: T,
    diagnostics: Vec<Diagnostic>,
    sink: Arc<Sink<T>>,
}

/// Where a section's events go. `active` is cleared when the subscription
/// is dropped, so a delivery already under way stops short.
struct Sink<T> {
    active: Arc<AtomicBool>,
    subscriber: Mutex<Box<dyn Fn(Event<T>) + Send>>,
}

impl Watcher {
    /// Starts watching the files of `sources`. Problems watching them (for
    /// example the inotify watch limit, reached by a directory created
    /// later) go to `on_error`, on the watcher's thread; the watcher
    /// carries on and reloads.
    ///
    /// # Errors
    ///
    /// [`WatchError`] when the file-watching backend or its thread cannot
    /// be started, or a directory cannot be watched.
    pub fn start(
        sources: Sources,
        on_error: impl Fn(WatchError) + Send + 'static,
    ) -> Result<Self, WatchError> {
        let targets = sources
            .files()
            .map(|file| Target::File(file.to_path_buf()))
            .collect();
        let armed = Armed::new(targets, Timing::default())?;
        let shared = Arc::new(Shared {
            sources,
            subscribers: Mutex::default(),
        });
        let worker = Arc::clone(&shared);
        let watch = armed.start("config-watch", move |event| match event {
            deskrinch_watch::Event::Changed => worker.reload(),
            deskrinch_watch::Event::Error(error) => on_error(error),
        })?;
        Ok(Self {
            _watch: watch,
            shared,
        })
    }

    /// Subscribes to `section`, and returns its settings now; the events
    /// `subscriber` hears are changes to them. `subscriber` is called on
    /// the watcher's thread, one event at a time.
    ///
    /// A reload that leaves the settings as they were delivers no
    /// [`Event::Changed`]; one that leaves the diagnostics as they were
    /// delivers no [`Event::Diagnostics`].
    pub fn subscribe<T: Settings>(
        &self,
        mut section: Section<T>,
        subscriber: impl Fn(Event<T>) + Send + 'static,
    ) -> (Subscription, Resolved<T>) {
        let active = Arc::new(AtomicBool::new(true));
        let mut subscribers = self.shared.lock();
        // Read under the lock: a reload either finished before, or reads
        // after and compares with this.
        let resolved = section.resolve(&Snapshot::read(&self.shared.sources));
        let typed = Typed {
            section,
            settings: resolved.settings.clone(),
            diagnostics: resolved.diagnostics.clone(),
            sink: Arc::new(Sink {
                active: Arc::clone(&active),
                subscriber: Mutex::new(Box::new(subscriber)),
            }),
        };
        let id = subscribers.next_id;
        subscribers.next_id += 1;
        subscribers.list.push((id, Box::new(typed)));
        let subscription = Subscription {
            shared: Arc::downgrade(&self.shared),
            id,
            active,
        };
        (subscription, resolved)
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Subscribers> {
        // A subscriber that panicked left nothing half-updated here.
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Reads the files once and delivers each section's changes. The
    /// deliveries run after the lock is released, so a subscriber may
    /// subscribe, or drop its subscription, from its callback.
    fn reload(&self) {
        let deliveries: Vec<Delivery> = {
            let mut subscribers = self.lock();
            let snapshot = Snapshot::read(&self.sources);
            subscribers
                .list
                .iter_mut()
                .filter_map(|(_, typed)| typed.reload(&snapshot))
                .collect()
        };
        for delivery in deliveries {
            delivery();
        }
    }
}

impl<T: Settings> Reload for Typed<T> {
    fn reload(&mut self, snapshot: &Snapshot) -> Option<Delivery> {
        let resolved = self.section.resolve(snapshot);
        let mut events = Vec::new();
        if resolved.diagnostics != self.diagnostics {
            self.diagnostics.clone_from(&resolved.diagnostics);
            events.push(Event::Diagnostics(resolved.diagnostics));
        }
        if resolved.settings != self.settings {
            self.settings.clone_from(&resolved.settings);
            events.push(Event::Changed(resolved.settings));
        }
        if events.is_empty() {
            return None;
        }
        let sink = Arc::clone(&self.sink);
        Some(Box::new(move || sink.deliver(events)))
    }
}

impl<T> Sink<T> {
    fn deliver(&self, events: Vec<Event<T>>) {
        let subscriber = self
            .subscriber
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for event in events {
            if !self.active.load(Ordering::Acquire) {
                return;
            }
            subscriber(event);
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        if let Some(shared) = self.shared.upgrade() {
            shared.lock().list.retain(|(id, _)| *id != self.id);
        }
    }
}

impl fmt::Debug for Watcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Watcher")
            .field("sources", &self.shared.sources)
            .field("subscriptions", &self.shared.lock().list.len())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription")
            .field("id", &self.id)
            .field("active", &self.active.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

//! The state the watcher object, the tracker, the worker and the handles
//! share: the model, the events waiting for the subscriber, and the work
//! (reads and calls) waiting for the tracker.

use std::collections::VecDeque;
use std::mem;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::Instant;

use crate::dbus::menu::ROOT;
use crate::host::{Activation, Event, OperationError};
use crate::item::{Item, ItemId};
use crate::menu::{EntryId, EntryKind, Menu, MenuEntry};
use crate::model::{Change, Due, MenuDue, MenuRefusal, MenuTarget, Model, Refusal, Target};

pub(crate) struct Shared {
    state: Mutex<State>,
    /// Wakes the worker when an event is queued or the host stops.
    wake: Condvar,
    inbox: Mutex<Inbox>,
}

struct State {
    model: Model,
    outbox: VecDeque<Event>,
    /// The host is stopping, or its connection has ended: the handles'
    /// operations report [`OperationError::Stopped`].
    stopped: bool,
    /// Only the worker's end: it has delivered everything and may return.
    stopping: bool,
}

/// A call the tracker makes for a handle.
#[derive(Debug, Clone)]
pub(crate) enum Call {
    /// `activation` to `target`, after `ProvideXdgActivationToken` with
    /// `token` if there is one.
    Activate {
        target: Target,
        activation: Activation,
        token: Option<String>,
    },
    /// The menu event `event` about `entry` to `target`, after
    /// `ProvideXdgActivationToken` with `token` if there is one.
    MenuEvent {
        target: MenuTarget,
        entry: i32,
        event: &'static str,
        token: Option<String>,
    },
    /// `AboutToShow` for `entry` of the item `id`'s open menu, whose answer
    /// may ask for the menu to be read again.
    AboutToShow { id: ItemId, entry: i32 },
}

/// The work the tracker is asked for.
#[derive(Debug, Default)]
pub(crate) struct Work {
    pub(crate) reads: Vec<Due>,
    pub(crate) menu_reads: Vec<MenuDue>,
    /// In the order the handles queued them.
    pub(crate) calls: Vec<Call>,
}

impl Work {
    fn is_empty(&self) -> bool {
        self.reads.is_empty() && self.menu_reads.is_empty() && self.calls.is_empty()
    }
}

fn menu_event(target: MenuTarget, entry: i32, event: &'static str, token: Option<String>) -> Call {
    Call::MenuEvent {
        target,
        entry,
        event,
        token,
    }
}

/// The handle's error for the model's refusal of an operation on the item
/// `id`'s menu, or on its `entry`.
fn menu_error(refusal: MenuRefusal, id: &ItemId, entry: Option<EntryId>) -> OperationError {
    match (refusal, entry) {
        (MenuRefusal::NotFound, _) => OperationError::NotFound(id.clone()),
        (MenuRefusal::NoMenu, _) | (MenuRefusal::NoEntry, None) => {
            OperationError::NoMenu(id.clone())
        }
        (MenuRefusal::NoEntry, Some(entry)) => OperationError::EntryUnavailable(entry),
    }
}

/// What the tracker is asked to do, and how to wake it.
#[derive(Default)]
struct Inbox {
    work: Work,
    stopped: bool,
    waker: Option<Waker>,
}

impl Shared {
    pub(crate) fn new(model: Model) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                model,
                outbox: VecDeque::new(),
                stopped: false,
                stopping: false,
            }),
            wake: Condvar::new(),
            inbox: Mutex::new(Inbox::default()),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_inbox(&self) -> MutexGuard<'_, Inbox> {
        self.inbox.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers an item and asks the tracker to read it; its id when it is
    /// new.
    pub(crate) fn register(&self, target: Target, owner: &str) -> Result<Option<ItemId>, Refusal> {
        let due = self.lock().model.register(target, owner, Instant::now())?;
        Ok(due.map(|due| {
            let id = due.id.clone();
            self.read(vec![due]);
            id
        }))
    }

    /// Registers another host; whether it is new.
    pub(crate) fn register_host(&self, service: &str) -> Result<bool, Refusal> {
        self.lock().model.register_host(service)
    }

    /// Runs `f` on the model; for the tracker.
    pub(crate) fn with_model<R>(&self, f: impl FnOnce(&mut Model) -> R) -> R {
        f(&mut self.lock().model)
    }

    pub(crate) fn registered(&self) -> Vec<String> {
        self.lock().model.registered()
    }

    pub(crate) fn items(&self) -> Vec<Arc<Item>> {
        self.lock().model.items()
    }

    pub(crate) fn item(&self, id: &ItemId) -> Option<Arc<Item>> {
        self.lock().model.item(id)
    }

    /// Queues `activation` of the item `id` for the tracker to send.
    pub(crate) fn activate(
        &self,
        id: &ItemId,
        activation: Activation,
        token: Option<&str>,
    ) -> Result<(), OperationError> {
        self.queue(|model| {
            let target = model
                .target(id)
                .ok_or_else(|| OperationError::NotFound(id.clone()))?;
            Ok(Work {
                calls: vec![Call::Activate {
                    target,
                    activation,
                    token: token.map(str::to_owned),
                }],
                ..Work::default()
            })
        })
    }

    /// Opens the item `id`'s menu: says so to the application, and asks
    /// the tracker to read it. Nothing when it is open already.
    pub(crate) fn open_menu(&self, id: &ItemId) -> Result<(), OperationError> {
        self.queue(|model| {
            let opened = model
                .open_menu(id, Instant::now())
                .map_err(|r| menu_error(r, id, None))?;
            Ok(opened.map_or_else(Work::default, |(target, due)| Work {
                menu_reads: vec![due],
                calls: vec![menu_event(target, ROOT, "opened", None)],
                ..Work::default()
            }))
        })
    }

    /// Closes the item `id`'s menu, and says so to the application.
    /// Nothing when it is not open.
    pub(crate) fn close_menu(&self, id: &ItemId) -> Result<(), OperationError> {
        self.queue(|model| {
            if model.item(id).is_none() {
                return Err(OperationError::NotFound(id.clone()));
            }
            Ok(Work {
                calls: model
                    .close_menu(id)
                    .map(|target| menu_event(target, ROOT, "closed", None))
                    .into_iter()
                    .collect(),
                ..Work::default()
            })
        })
    }

    /// Says to the application that the submenu of `entry` opened, with
    /// `AboutToShow`, whose answer may ask for the menu to be read again.
    pub(crate) fn open_submenu(&self, id: &ItemId, entry: EntryId) -> Result<(), OperationError> {
        self.queue(|model| {
            let target = model
                .entry_target(id, entry, |e| e.submenu.is_some())
                .map_err(|r| menu_error(r, id, Some(entry)))?;
            Ok(Work {
                // As on opening the menu itself: the event, then the call.
                calls: vec![
                    menu_event(target, entry.id, "opened", None),
                    Call::AboutToShow {
                        id: id.clone(),
                        entry: entry.id,
                    },
                ],
                ..Work::default()
            })
        })
    }

    /// Says to the application that the submenu of `entry` closed.
    pub(crate) fn close_submenu(&self, id: &ItemId, entry: EntryId) -> Result<(), OperationError> {
        self.queue(|model| {
            let target = model
                .entry_target(id, entry, |e| e.submenu.is_some())
                .map_err(|r| menu_error(r, id, Some(entry)))?;
            Ok(Work {
                calls: vec![menu_event(target, entry.id, "closed", None)],
                ..Work::default()
            })
        })
    }

    /// Activates `entry` of the item `id`'s open menu, if it is still the
    /// entry the caller was given and can be activated.
    pub(crate) fn activate_entry(
        &self,
        id: &ItemId,
        entry: EntryId,
        token: Option<&str>,
    ) -> Result<(), OperationError> {
        self.queue(|model| {
            let activatable = |e: &MenuEntry| e.kind == EntryKind::Standard && e.enabled;
            let target = model
                .entry_target(id, entry, activatable)
                .map_err(|r| menu_error(r, id, Some(entry)))?;
            Ok(Work {
                calls: vec![menu_event(
                    target,
                    entry.id,
                    "clicked",
                    token.map(str::to_owned),
                )],
                ..Work::default()
            })
        })
    }

    /// The item `id`'s open menu, as last read.
    pub(crate) fn menu(&self, id: &ItemId) -> Option<Arc<Menu>> {
        self.lock().model.menu(id)
    }

    /// Asks the model for the work an operation queues, and queues it for
    /// the tracker, in order with what is queued already; the operation's
    /// error, or that the host has stopped.
    fn queue(
        &self,
        operation: impl FnOnce(&mut Model) -> Result<Work, OperationError>,
    ) -> Result<(), OperationError> {
        // The inbox is held from the model's decision to the queueing, so
        // that two handles' operations are queued in the order the model
        // saw them; the tracker never takes the state's lock while it holds
        // the inbox's.
        let mut inbox = self.lock_inbox();
        let work = {
            let mut state = self.lock();
            if state.stopped || inbox.stopped {
                return Err(OperationError::Stopped);
            }
            operation(&mut state.model)?
        };
        if work.is_empty() {
            return Ok(());
        }
        inbox.work.reads.extend(work.reads);
        inbox.work.menu_reads.extend(work.menu_reads);
        inbox.work.calls.extend(work.calls);
        if let Some(waker) = inbox.waker.take() {
            waker.wake();
        }
        Ok(())
    }

    /// Queues the events for `changes`, and `extra` after them.
    pub(crate) fn report(&self, changes: impl IntoIterator<Item = Change>, extra: Option<Event>) {
        let mut state = self.lock();
        let before = state.outbox.len();
        state.outbox.extend(changes.into_iter().map(Event::from));
        state.outbox.extend(extra);
        if state.outbox.len() > before {
            self.wake.notify_all();
        }
    }

    /// The connection ended: the subscriber hears it, and the handles'
    /// operations report that the host has stopped.
    pub(crate) fn disconnected(&self) {
        self.lock().stopped = true;
        // Calls queued meanwhile could not be sent: they go.
        self.stop_tracker();
        self.report([], Some(Event::Disconnected));
    }

    /// Asks the tracker to start `reads`.
    fn read(&self, reads: Vec<Due>) {
        if reads.is_empty() {
            return;
        }
        let mut inbox = self.lock_inbox();
        inbox.work.reads.extend(reads);
        if let Some(waker) = inbox.waker.take() {
            waker.wake();
        }
    }

    /// For the tracker: the work it was asked for, or `None` once it is to
    /// stop; `Pending` with `cx` woken when either comes.
    pub(crate) fn poll_inbox(&self, cx: &Context<'_>) -> Poll<Option<Work>> {
        let mut inbox = self.lock_inbox();
        if inbox.stopped {
            return Poll::Ready(None);
        }
        if !inbox.work.is_empty() {
            return Poll::Ready(Some(mem::take(&mut inbox.work)));
        }
        inbox.waker = Some(cx.waker().clone());
        Poll::Pending
    }

    /// Tells the tracker to stop, dropping the work it has not taken.
    pub(crate) fn stop_tracker(&self) {
        let mut inbox = self.lock_inbox();
        inbox.stopped = true;
        inbox.work = Work::default();
        if let Some(waker) = inbox.waker.take() {
            waker.wake();
        }
    }

    /// For the worker: the next events, waiting for them; `None` once the
    /// host is stopping and all were delivered.
    pub(crate) fn next_events(&self) -> Option<Vec<Event>> {
        let mut state = self.lock();
        loop {
            if !state.outbox.is_empty() {
                return Some(mem::take(&mut state.outbox).into());
            }
            if state.stopping {
                return None;
            }
            state = self
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The host is stopping: the handles report it, and the worker returns
    /// once it has delivered what is queued.
    pub(crate) fn stop(&self) {
        let mut state = self.lock();
        state.stopped = true;
        state.stopping = true;
        self.wake.notify_all();
    }
}

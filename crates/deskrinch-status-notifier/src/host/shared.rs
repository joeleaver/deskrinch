//! The state the watcher object, the tracker, the worker and the handles
//! share: the model, the events waiting for the subscriber, and the reads
//! waiting for the tracker.

use std::collections::VecDeque;
use std::mem;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};

use crate::host::{Event, OperationError};
use crate::item::{Item, ItemId};
use crate::model::{Change, Model, Refusal, Target};

pub(crate) struct Shared {
    state: Mutex<State>,
    /// Wakes the worker when an event is queued or the host stops.
    wake: Condvar,
    inbox: Mutex<Inbox>,
}

struct State {
    model: Model,
    outbox: VecDeque<Event>,
    stopping: bool,
}

/// What the tracker is asked to do, and how to wake it.
#[derive(Default)]
struct Inbox {
    reads: Vec<(ItemId, Target)>,
    stopped: bool,
    waker: Option<Waker>,
}

impl Shared {
    pub(crate) fn new(model: Model) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                model,
                outbox: VecDeque::new(),
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
        let registered = self.lock().model.register(target, owner)?;
        Ok(registered.map(|(id, target)| {
            self.read(vec![(id.clone(), target)]);
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

    /// Where to call the item `id`, if the caller may.
    pub(crate) fn target(&self, id: &ItemId) -> Result<Target, OperationError> {
        let state = self.lock();
        if state.stopping {
            return Err(OperationError::Stopped);
        }
        state
            .model
            .target(id)
            .ok_or_else(|| OperationError::NotFound(id.clone()))
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

    /// Asks the tracker to start `reads`.
    pub(crate) fn read(&self, reads: Vec<(ItemId, Target)>) {
        if reads.is_empty() {
            return;
        }
        let mut inbox = self.lock_inbox();
        inbox.reads.extend(reads);
        if let Some(waker) = inbox.waker.take() {
            waker.wake();
        }
    }

    /// For the tracker: the reads it was asked for, or `None` once it is to
    /// stop; `Pending` with `cx` woken when either comes.
    pub(crate) fn poll_inbox(&self, cx: &Context<'_>) -> Poll<Option<Vec<(ItemId, Target)>>> {
        let mut inbox = self.lock_inbox();
        if inbox.stopped {
            return Poll::Ready(None);
        }
        if !inbox.reads.is_empty() {
            return Poll::Ready(Some(mem::take(&mut inbox.reads)));
        }
        inbox.waker = Some(cx.waker().clone());
        Poll::Pending
    }

    /// Tells the tracker to stop.
    pub(crate) fn stop_tracker(&self) {
        let mut inbox = self.lock_inbox();
        inbox.stopped = true;
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

    /// Tells the worker to stop once it has delivered what is queued.
    pub(crate) fn stop(&self) {
        self.lock().stopping = true;
        self.wake.notify_all();
    }
}

//! The state the watcher object, the tracker, the worker and the handles
//! share: the model, the events waiting for the subscriber, and the work
//! (reads and calls) waiting for the tracker.

use std::collections::VecDeque;
use std::mem;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::Instant;

use crate::host::{Activation, Event, OperationError};
use crate::item::{Item, ItemId};
use crate::model::{Change, Due, Model, Refusal, Target};

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

/// A call the tracker sends for a handle: `activation` to `target`, after
/// `ProvideXdgActivationToken` with `token` if there is one.
#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub(crate) target: Target,
    pub(crate) activation: Activation,
    pub(crate) token: Option<String>,
}

/// The work the tracker is asked for.
#[derive(Debug, Default)]
pub(crate) struct Work {
    pub(crate) reads: Vec<Due>,
    /// In the order the handles queued them.
    pub(crate) calls: Vec<Call>,
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
        let target = {
            let state = self.lock();
            if state.stopped {
                return Err(OperationError::Stopped);
            }
            state
                .model
                .target(id)
                .ok_or_else(|| OperationError::NotFound(id.clone()))?
        };
        let mut inbox = self.lock_inbox();
        if inbox.stopped {
            return Err(OperationError::Stopped);
        }
        inbox.work.calls.push(Call {
            target,
            activation,
            token: token.map(str::to_owned),
        });
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
    pub(crate) fn read(&self, reads: Vec<Due>) {
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
        if !inbox.work.reads.is_empty() || !inbox.work.calls.is_empty() {
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

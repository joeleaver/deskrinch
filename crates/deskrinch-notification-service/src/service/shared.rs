//! The state the D-Bus object, the worker and the handles share.
//!
//! Every change to the model goes through here, under one lock, and leaves
//! its outputs (events for the subscriber, signals for the bus) in one queue
//! in the order the changes happened. Only the worker drains the queue, so
//! the subscriber sees changes in order and is never called with the lock
//! held: it may call back into a [`Handle`](super::Handle).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::task::Wake;

use crate::clock::Clock;
use crate::model::{ActionRefused, Change, Model, Refused};
use crate::notification::{CloseReason, Notification, NotificationId};
use crate::request::{Policy, Request};
use crate::service::{Event, OperationError, ServerInfo};

/// A signal for the bus, emitted by the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Signal {
    NotificationClosed {
        id: NotificationId,
        reason: CloseReason,
    },
    ActionInvoked {
        id: NotificationId,
        key: String,
    },
    ActivationToken {
        id: NotificationId,
        token: String,
    },
}

/// The service is stopping and takes no more changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stopped;

impl From<Stopped> for OperationError {
    fn from(Stopped: Stopped) -> Self {
        Self::Stopped
    }
}

/// Why a `Notify` call made no notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotifyError {
    Refused(Refused),
    Stopped,
}

impl From<Stopped> for NotifyError {
    fn from(Stopped: Stopped) -> Self {
        Self::Stopped
    }
}

/// Something the worker delivers.
#[derive(Debug, Clone)]
pub(crate) enum Output {
    Event(Event),
    Signal(Signal),
}

pub(crate) struct State {
    pub(crate) model: Model,
    pub(crate) outbox: VecDeque<Output>,
    pub(crate) stopping: bool,
}

impl State {
    /// Queues the outputs of `change`.
    pub(crate) fn record(&mut self, change: Change) {
        match change {
            Change::Shown(n) => self.outbox.push_back(Output::Event(Event::Shown(n))),
            Change::Replaced(n) => self.outbox.push_back(Output::Event(Event::Replaced(n))),
            Change::Closed { id, reason } => {
                self.outbox
                    .push_back(Output::Event(Event::Closed { id, reason }));
                self.outbox
                    .push_back(Output::Signal(Signal::NotificationClosed { id, reason }));
            }
            Change::ActionInvoked {
                id,
                key,
                activation_token,
            } => {
                // The specification: the token comes before ActionInvoked.
                if let Some(token) = activation_token {
                    self.outbox
                        .push_back(Output::Signal(Signal::ActivationToken { id, token }));
                }
                self.outbox
                    .push_back(Output::Signal(Signal::ActionInvoked { id, key }));
            }
        }
    }
}

pub(crate) struct Shared {
    state: Mutex<State>,
    /// Woken when the outbox gains something, the clock jumps, or the
    /// service stops.
    pub(crate) wake: Condvar,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) server: ServerInfo,
    pub(crate) owns_name: AtomicBool,
}

impl Shared {
    pub(crate) fn new(policy: Policy, clock: Arc<dyn Clock>, server: ServerInfo) -> Arc<Self> {
        let shared = Arc::new(Self {
            state: Mutex::new(State {
                model: Model::new(policy),
                outbox: VecDeque::new(),
                stopping: false,
            }),
            wake: Condvar::new(),
            clock,
            server,
            owns_name: AtomicBool::new(false),
        });
        let waker = Arc::new(ClockWaker(Arc::downgrade(&shared)));
        shared.clock.wake_on_advance(waker.into());
        shared
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `f` on the state unless the service is stopping, then wakes the
    /// worker.
    fn change<T, E: From<Stopped>>(
        &self,
        f: impl FnOnce(&mut State) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut state = self.lock();
        if state.stopping {
            return Err(Stopped.into());
        }
        let result = f(&mut state);
        drop(state);
        self.wake.notify_all();
        result
    }

    /// A `Notify` call.
    pub(crate) fn notify(&self, request: &Request<'_>) -> Result<NotificationId, NotifyError> {
        let now = self.clock.now();
        self.change(|state| {
            let (id, change) = state
                .model
                .notify(request, now)
                .map_err(NotifyError::Refused)?;
            state.record(change);
            Ok(id)
        })
    }

    /// Pauses (`true`) or resumes `id`'s timer; the worker then waits for
    /// the new next expiry.
    pub(crate) fn set_paused(
        &self,
        id: NotificationId,
        paused: bool,
    ) -> Result<(), OperationError> {
        let now = self.clock.now();
        self.change(|state| {
            let done = if paused {
                state.model.pause(id, now)
            } else {
                state.model.resume(id, now)
            };
            done.ok_or(OperationError::NotFound(id))
        })
    }

    /// Closes `id` for `reason`.
    pub(crate) fn close(
        &self,
        id: NotificationId,
        reason: CloseReason,
    ) -> Result<(), OperationError> {
        self.change(|state| {
            let change = state
                .model
                .close(id, reason)
                .ok_or(OperationError::NotFound(id))?;
            state.record(change);
            Ok(())
        })
    }

    pub(crate) fn invoke_action(
        &self,
        id: NotificationId,
        key: &str,
        activation_token: Option<String>,
    ) -> Result<(), OperationError> {
        self.change(|state| {
            let changes = state
                .model
                .invoke_action(id, key, activation_token)
                .map_err(|refused| match refused {
                    ActionRefused::NotFound => OperationError::NotFound(id),
                    ActionRefused::UnknownAction => OperationError::UnknownAction {
                        id,
                        key: key.to_owned(),
                    },
                })?;
            for change in changes {
                state.record(change);
            }
            Ok(())
        })
    }

    pub(crate) fn notifications(&self) -> Vec<Arc<Notification>> {
        self.lock().model.live().to_vec()
    }

    pub(crate) fn notification(&self, id: NotificationId) -> Option<Arc<Notification>> {
        self.lock().model.get(id).cloned()
    }

    pub(crate) fn capabilities(&self) -> Vec<&'static str> {
        self.lock().model.policy().capabilities.names()
    }

    /// Queues an event that is not a model change (name ownership).
    pub(crate) fn push_event(&self, event: Event) {
        self.lock().outbox.push_back(Output::Event(event));
        self.wake.notify_all();
    }

    pub(crate) fn set_owns_name(&self, owns: bool) {
        self.owns_name.store(owns, Ordering::SeqCst);
    }

    /// Tells the worker to deliver what is queued and stop.
    pub(crate) fn stop(&self) {
        self.lock().stopping = true;
        self.wake.notify_all();
    }
}

/// Wakes the worker when a [`ManualClock`](crate::ManualClock) advances.
struct ClockWaker(Weak<Shared>);

impl Wake for ClockWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if let Some(shared) = self.0.upgrade() {
            // Taking the lock orders this wake after the worker's last look
            // at the clock, so it cannot be missed.
            drop(shared.lock());
            shared.wake.notify_all();
        }
    }
}

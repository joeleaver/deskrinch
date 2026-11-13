//! The states on a connection, as the client's thread keeps them: attached
//! ones, and releasing ones (detached, still dispatched until nothing can
//! reach them).

use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{DispatchError, EventQueue};

use crate::globals::{GlobalEvent, Globals, Registry};
use crate::lifecycle::{EndCause, Lifecycle};

/// An attached state, with its queue, behind one type.
pub(crate) struct Attached {
    pub(crate) id: u64,
    pub(crate) live: Box<dyn Live>,
}

/// What the thread does with a state.
pub(crate) trait Live: Send {
    /// Dispatches the events its queue holds; how many.
    fn dispatch(&mut self) -> Result<usize, DispatchError>;
    /// Hands it a registry event.
    fn global(&mut self, registry: &WlRegistry, event: &GlobalEvent);
    /// Ends it, once.
    fn end(&mut self, cause: EndCause);
    /// Releases what it bound, once.
    fn release(&mut self);
    /// Whether nothing it bound can reach it any more.
    fn released(&self) -> bool;
}

/// A state and the queue dispatched into it.
pub(crate) struct Queue<State> {
    pub(crate) queue: EventQueue<State>,
    pub(crate) state: State,
}

impl<State: Lifecycle + Globals + Send + 'static> Live for Queue<State> {
    fn dispatch(&mut self) -> Result<usize, DispatchError> {
        self.queue.dispatch_pending(&mut self.state)
    }

    fn global(&mut self, registry: &WlRegistry, event: &GlobalEvent) {
        match event {
            GlobalEvent::Added(global) => {
                self.state
                    .global_added(registry, global, &self.queue.handle());
            }
            GlobalEvent::Removed(name) => self.state.global_removed(*name),
        }
    }

    fn end(&mut self, cause: EndCause) {
        self.state.end(cause);
    }

    fn release(&mut self) {
        self.state.release();
    }

    fn released(&self) -> bool {
        self.state.released()
    }
}

/// How many states a connection has, as the client's thread last left
/// them; see [`Client::states`](crate::Client::states).
#[derive(Debug, Default)]
pub(crate) struct Counts {
    attached: AtomicUsize,
    releasing: AtomicUsize,
    discarded: AtomicU64,
}

/// How many states a [`Client`](crate::Client)'s connection has, and how
/// much traffic its releasing ones have had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateCounts {
    /// States attached: their [`Attachment`](crate::Attachment) is held.
    pub attached: usize,
    /// States released but still dispatched: their attachment was dropped,
    /// and something they bound can still send them events (a manager not
    /// yet `finished`, or an object without a destructor, which keeps them
    /// for as long as the connection lasts).
    pub releasing: usize,
    /// The events dispatched into states after their release, over the
    /// client's life: traffic for nobody, consumed as it came rather than
    /// kept. Under KWin, whose window-management and virtual-desktop
    /// objects have no destructor, it keeps growing after a detach on a
    /// shared connection, which makes it worth watching there.
    pub discarded: u64,
}

impl Counts {
    /// The counts now.
    pub(crate) fn get(&self) -> StateCounts {
        StateCounts {
            attached: self.attached.load(Ordering::Acquire),
            releasing: self.releasing.load(Ordering::Acquire),
            discarded: self.discarded.load(Ordering::Acquire),
        }
    }
}

/// A handler (or a setup, a release or a drop) panicked.
pub(crate) struct Panicked;

/// The states on the connection.
pub(crate) struct States {
    attached: Vec<Attached>,
    releasing: Vec<Box<dyn Live>>,
    counts: Arc<Counts>,
}

impl States {
    /// None yet, counted in `counts`.
    pub(crate) fn new(counts: Arc<Counts>) -> Self {
        Self {
            attached: Vec::new(),
            releasing: Vec::new(),
            counts,
        }
    }

    /// Attaches `new`.
    pub(crate) fn attach(&mut self, new: Attached) {
        self.attached.push(new);
        self.count();
    }

    /// Detaches state `id`: it releases what it bound, and is dropped at
    /// once if nothing it bound can reach it any more, or kept releasing.
    pub(crate) fn detach(&mut self, id: u64) -> Result<(), Panicked> {
        let Some(index) = self.attached.iter().position(|state| state.id == id) else {
            return Ok(());
        };
        let mut detached = self.attached.remove(index);
        let released = panic::catch_unwind(AssertUnwindSafe(|| {
            detached.live.release();
            detached.live.released()
        }));
        let outcome = match released {
            Ok(false) => {
                self.releasing.push(detached.live);
                Ok(())
            }
            Ok(true) => drop_state(detached.live),
            Err(_) => Err(Panicked),
        };
        self.count();
        outcome
    }

    /// Dispatches every attached state's queue, then every releasing one's
    /// (dropping those that nothing can reach any more), then hands the
    /// registry's events to the attached states; why the connection ends,
    /// if it does.
    ///
    /// The registry's events come last: a global's removal is sent after
    /// the events its object was sent before it, and read with them or
    /// later, so a state hears those events before the removal. (An added
    /// global has no object yet, so nothing of it can be waiting.) A
    /// releasing state hears nothing of the registry.
    pub(crate) fn dispatch(&mut self, registry: &mut Registry) -> Option<EndCause> {
        for state in &mut self.attached {
            match panic::catch_unwind(AssertUnwindSafe(|| state.live.dispatch())) {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => return Some(EndCause::ConnectionLost(Arc::new(error))),
                Err(_) => return Some(EndCause::HandlerPanicked),
            }
        }
        if let Some(cause) = self.dispatch_releasing() {
            return Some(cause);
        }
        self.forward_globals(registry)
    }

    /// Dispatches the releasing states, counting what they are handed, and
    /// drops those that nothing can reach any more.
    fn dispatch_releasing(&mut self) -> Option<EndCause> {
        let mut index = 0;
        while index < self.releasing.len() {
            let state = &mut self.releasing[index];
            let dispatched =
                panic::catch_unwind(AssertUnwindSafe(|| (state.dispatch(), state.released())));
            match dispatched {
                Ok((Ok(events), released)) => {
                    self.counts
                        .discarded
                        .fetch_add(u64::try_from(events).unwrap_or(u64::MAX), Ordering::AcqRel);
                    if released {
                        let done = self.releasing.swap_remove(index);
                        self.count();
                        if drop_state(done).is_err() {
                            return Some(EndCause::HandlerPanicked);
                        }
                        continue;
                    }
                }
                Ok((Err(error), _)) => return Some(EndCause::ConnectionLost(Arc::new(error))),
                Err(_) => return Some(EndCause::HandlerPanicked),
            }
            index += 1;
        }
        None
    }

    /// Hands the registry events read so far to every attached state, in
    /// order; why the connection ends, if it does.
    fn forward_globals(&mut self, registry: &mut Registry) -> Option<EndCause> {
        let events = match registry.take() {
            Ok(events) => events,
            Err(error) => return Some(EndCause::ConnectionLost(Arc::new(error))),
        };
        let wl_registry = registry.globals().registry();
        for event in &events {
            for state in &mut self.attached {
                let handed = panic::catch_unwind(AssertUnwindSafe(|| {
                    state.live.global(wl_registry, event);
                }));
                if handed.is_err() {
                    return Some(EndCause::HandlerPanicked);
                }
            }
        }
        None
    }

    /// The connection is over: every attached state is ended once, and
    /// every state dropped. A releasing state is dropped as it is: nothing
    /// is sent on a dead connection, and it has no one to tell.
    pub(crate) fn end(&mut self, cause: &EndCause) {
        for mut state in self.attached.drain(..) {
            // A state whose end panics has had its end; the others still get
            // theirs.
            drop(panic::catch_unwind(AssertUnwindSafe(|| {
                state.live.end(cause.clone());
            })));
        }
        for state in self.releasing.drain(..) {
            drop(drop_state(state));
        }
        self.count();
    }

    /// Publishes the counts.
    fn count(&self) {
        let counts = &self.counts;
        counts
            .attached
            .store(self.attached.len(), Ordering::Release);
        counts
            .releasing
            .store(self.releasing.len(), Ordering::Release);
    }
}

/// Drops a state; a drop that panics is a handler panicking.
fn drop_state(state: Box<dyn Live>) -> Result<(), Panicked> {
    panic::catch_unwind(AssertUnwindSafe(|| drop(state))).map_err(|_| Panicked)
}

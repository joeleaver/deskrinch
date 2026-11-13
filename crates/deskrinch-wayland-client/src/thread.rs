//! The client's thread: the only reader of the connection, and the
//! dispatcher of every queue attached to it.

use std::io::ErrorKind;
use std::os::fd::{AsFd, OwnedFd};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender};

use rustix::event::{PollFd, PollFlags, poll};
use wayland_client::backend::WaylandError;
use wayland_client::globals::GlobalList;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, DispatchError, EventQueue};

use crate::ended::{EndCause, Ended};
use crate::globals::{GlobalEvent, Globals, Registry};

/// What other threads ask of the client's thread.
pub(crate) enum Control {
    /// Run an attachment's setup and, if it succeeds, dispatch its queue.
    Attach(Job),
    /// Stop dispatching attachment `id` and drop it; acknowledge, when
    /// asked, once that is done.
    Detach(u64, Option<SyncSender<()>>),
    /// The last handle to the client is gone: end the thread.
    Stop,
}

/// An attachment's setup, run on the client's thread with the connection
/// and its globals (or with `None` once the connection has ended, to
/// refuse). It answers its waiter itself, and returns what to dispatch.
pub(crate) type Job =
    Box<dyn FnOnce(Option<(&Connection, &GlobalList)>) -> Option<Attached> + Send>;

/// An attached state, with its queue, behind one type.
pub(crate) struct Attached {
    pub(crate) id: u64,
    pub(crate) live: Box<dyn Live>,
}

/// What the thread does with an attached state.
pub(crate) trait Live: Send {
    /// Dispatches the events its queue holds.
    fn dispatch(&mut self) -> Result<usize, DispatchError>;
    /// Hands it a registry event.
    fn global(&mut self, registry: &WlRegistry, event: &GlobalEvent);
    /// Ends it, once.
    fn end(&mut self, cause: EndCause);
}

/// A state and the queue dispatched into it.
pub(crate) struct Queue<State> {
    pub(crate) queue: EventQueue<State>,
    pub(crate) state: State,
}

impl<State: Ended + Globals + Send + 'static> Live for Queue<State> {
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
}

/// The thread's body.
pub(crate) fn run(
    connection: &Connection,
    mut registry: Registry,
    controls: &Receiver<Control>,
    wake: &OwnedFd,
) {
    let mut attached: Vec<Attached> = Vec::new();
    let cause = loop {
        match take_controls(connection, &mut registry, controls, &mut attached) {
            Next::Go => {}
            Next::End(cause) => break cause,
            // The last handle is gone, and with it every attachment.
            Next::Stop => return,
        }
        if let Some(cause) = dispatch_all(&mut registry, &mut attached) {
            break cause;
        }
        // Whether requests are left unsent: the socket would not take them,
        // so the thread also waits for it to be writable.
        let unsent = match connection.flush() {
            Ok(()) => false,
            // The compositor is slow to read: not an error (the backend does
            // not record it as one). Try again once the socket is writable.
            Err(WaylandError::Io(error)) if error.kind() == ErrorKind::WouldBlock => true,
            Err(error) => break lost(error),
        };
        // The connection's only reader: no other thread can take events
        // meant for a queue this thread is about to wait for.
        let Some(guard) = connection.prepare_read() else {
            // The backend's own queue holds events (the system backend):
            // sort them into the attached queues here, whether or not any
            // state is attached, so that this cannot spin.
            if let Err(error) = connection.backend().dispatch_inner_queue() {
                break lost(error);
            }
            continue;
        };
        let (readable, woken) = {
            let socket = guard.connection_fd();
            let mut wanted = PollFlags::IN | PollFlags::ERR;
            if unsent {
                wanted |= PollFlags::OUT;
            }
            let mut fds = [
                PollFd::new(&socket, wanted),
                PollFd::new(wake, PollFlags::IN),
            ];
            match poll(&mut fds, None) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => break lost(WaylandError::Io(error.into())),
            }
            let socket = fds[0].revents();
            (
                socket.intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP),
                !fds[1].revents().is_empty(),
            )
        };
        if woken {
            drain(wake);
        }
        if readable {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(error)) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => break lost(error),
            }
        } else {
            // Woken for a control, or the socket became writable: no read
            // this time; the next round flushes.
            drop(guard);
        }
    };
    end_all(&mut attached, &cause);
    refuse_until_stopped(controls);
}

/// What the thread does after taking the controls.
enum Next {
    /// Dispatch, and wait.
    Go,
    /// End every state, for this cause.
    End(EndCause),
    /// Stop: the last handle is gone.
    Stop,
}

/// Carries out what other threads asked, first in every round: a detached
/// state is never dispatched again. A setup or a detached state's drop that
/// panics ends the connection.
///
/// Before a setup, everything read so far is dispatched to the states
/// already attached, the registry's events included: the new state's
/// setup lists the globals as they are then, and hears of those read from
/// then on.
fn take_controls(
    connection: &Connection,
    registry: &mut Registry,
    controls: &Receiver<Control>,
    attached: &mut Vec<Attached>,
) -> Next {
    while let Ok(control) = controls.try_recv() {
        match control {
            Control::Attach(job) => {
                if let Some(cause) = dispatch_all(registry, attached) {
                    return Next::End(cause);
                }
                let globals = registry.globals();
                match panic::catch_unwind(AssertUnwindSafe(|| job(Some((connection, globals))))) {
                    Ok(Some(new)) => attached.push(new),
                    Ok(None) => {}
                    // A setup's handler panicked (its waiter has heard the
                    // end already: the answer was dropped unsent).
                    Err(_) => return Next::End(EndCause::HandlerPanicked),
                }
            }
            Control::Detach(id, acknowledge) => {
                let detached = attached
                    .iter()
                    .position(|attachment| attachment.id == id)
                    .map(|index| attached.remove(index));
                // A state whose drop panics is a handler panicking.
                let dropped = panic::catch_unwind(AssertUnwindSafe(|| drop(detached)));
                if let Some(acknowledge) = acknowledge {
                    // The waiter may be gone; nothing else to tell.
                    let _ = acknowledge.send(());
                }
                if dropped.is_err() {
                    return Next::End(EndCause::HandlerPanicked);
                }
            }
            Control::Stop => return Next::Stop,
        }
    }
    Next::Go
}

/// Hands the registry events read so far to every attached state, in
/// order; why the connection ends, if it does.
fn forward_globals(registry: &mut Registry, attached: &mut [Attached]) -> Option<EndCause> {
    let events = match registry.take() {
        Ok(events) => events,
        Err(error) => return Some(EndCause::ConnectionLost(Arc::new(error))),
    };
    let wl_registry = registry.globals().registry();
    for event in &events {
        for attachment in attached.iter_mut() {
            let handed = panic::catch_unwind(AssertUnwindSafe(|| {
                attachment.live.global(wl_registry, event);
            }));
            if handed.is_err() {
                return Some(EndCause::HandlerPanicked);
            }
        }
    }
    None
}

/// Dispatches every attached queue, then hands on the registry events; why
/// the connection ends, if it does.
///
/// The registry's events come last: a global's removal is sent after the
/// events its object was sent before it, and read with them or later, so a
/// state hears those events before the removal. (An added global has no
/// object yet, so nothing of it can be waiting.)
fn dispatch_all(registry: &mut Registry, attached: &mut [Attached]) -> Option<EndCause> {
    for attachment in attached.iter_mut() {
        match panic::catch_unwind(AssertUnwindSafe(|| attachment.live.dispatch())) {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => return Some(EndCause::ConnectionLost(Arc::new(error))),
            Err(_) => return Some(EndCause::HandlerPanicked),
        }
    }
    forward_globals(registry, attached)
}

/// Ends every attached state once, and drops them all.
fn end_all(attached: &mut Vec<Attached>, cause: &EndCause) {
    for mut attachment in attached.drain(..) {
        // A state whose end panics has had its end; the others still get
        // theirs.
        drop(panic::catch_unwind(AssertUnwindSafe(|| {
            attachment.live.end(cause.clone());
        })));
    }
}

/// After the end: answer every control until the last handle is gone, so
/// that no waiter waits for a thread that will not dispatch again.
fn refuse_until_stopped(controls: &Receiver<Control>) {
    while let Ok(control) = controls.recv() {
        match control {
            Control::Attach(job) => {
                drop(panic::catch_unwind(AssertUnwindSafe(|| job(None))));
            }
            Control::Detach(_, acknowledge) => {
                if let Some(acknowledge) = acknowledge {
                    let _ = acknowledge.send(());
                }
            }
            Control::Stop => return,
        }
    }
}

/// The cause for a failed read, write or poll.
fn lost(error: WaylandError) -> EndCause {
    EndCause::ConnectionLost(Arc::new(DispatchError::Backend(error)))
}

/// Empties the wake-up eventfd.
fn drain(wake: &OwnedFd) {
    let mut count = [0_u8; 8];
    // Non-blocking: nothing to read means nothing to drain.
    let _ = rustix::io::read(wake.as_fd(), &mut count);
}

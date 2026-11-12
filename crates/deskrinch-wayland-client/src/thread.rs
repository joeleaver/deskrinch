//! The client's thread: the only reader of the connection, and the
//! dispatcher of every queue attached to it.

use std::io::ErrorKind;
use std::os::fd::{AsFd, OwnedFd};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender};

use rustix::event::{PollFd, PollFlags, poll};
use wayland_client::backend::WaylandError;
use wayland_client::{Connection, DispatchError, EventQueue};

use crate::ended::{EndCause, Ended};

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
/// (or with `None` once the connection has ended, to refuse). It answers
/// its waiter itself, and returns what to dispatch.
pub(crate) type Job = Box<dyn FnOnce(Option<&Connection>) -> Option<Attached> + Send>;

/// An attached state, with its queue, behind one type.
pub(crate) struct Attached {
    pub(crate) id: u64,
    pub(crate) live: Box<dyn Live>,
}

/// What the thread does with an attached state.
pub(crate) trait Live: Send {
    /// Dispatches the events its queue holds.
    fn dispatch(&mut self) -> Result<usize, DispatchError>;
    /// Ends it, once.
    fn end(&mut self, cause: EndCause);
}

/// A state and the queue dispatched into it.
pub(crate) struct Queue<State> {
    pub(crate) queue: EventQueue<State>,
    pub(crate) state: State,
}

impl<State: Ended + Send + 'static> Live for Queue<State> {
    fn dispatch(&mut self) -> Result<usize, DispatchError> {
        self.queue.dispatch_pending(&mut self.state)
    }

    fn end(&mut self, cause: EndCause) {
        self.state.end(cause);
    }
}

/// The thread's body.
pub(crate) fn run(connection: &Connection, controls: &Receiver<Control>, wake: &OwnedFd) {
    let mut attached: Vec<Attached> = Vec::new();
    let cause = loop {
        // What other threads asked, first: a detached state is never
        // dispatched again.
        while let Ok(control) = controls.try_recv() {
            match control {
                Control::Attach(job) => {
                    match panic::catch_unwind(AssertUnwindSafe(|| job(Some(connection)))) {
                        Ok(Some(new)) => attached.push(new),
                        Ok(None) => {}
                        // A setup's handler panicked (its waiter has heard the
                        // end already: the answer was dropped unsent).
                        Err(_) => {
                            end_all(&mut attached, &EndCause::HandlerPanicked);
                            refuse_until_stopped(controls);
                            return;
                        }
                    }
                }
                Control::Detach(id, acknowledge) => {
                    attached.retain(|attachment| attachment.id != id);
                    if let Some(acknowledge) = acknowledge {
                        // The waiter may be gone; nothing else to tell.
                        let _ = acknowledge.send(());
                    }
                }
                Control::Stop => return,
            }
        }
        if let Some(cause) = dispatch_all(&mut attached) {
            break cause;
        }
        if let Err(error) = connection.flush() {
            break lost(error);
        }
        // The connection's only reader: no other thread can take events
        // meant for a queue this thread is about to wait for.
        let Some(guard) = connection.prepare_read() else {
            // Events are already queued (the system backend's own queue):
            // dispatch them first.
            continue;
        };
        let (socket, woken) = {
            let socket = guard.connection_fd();
            let mut fds = [
                PollFd::new(&socket, PollFlags::IN | PollFlags::ERR),
                PollFd::new(wake, PollFlags::IN),
            ];
            match poll(&mut fds, None) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => break lost(WaylandError::Io(error.into())),
            }
            (!fds[0].revents().is_empty(), !fds[1].revents().is_empty())
        };
        if woken {
            drain(wake);
        }
        if socket {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(error)) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => break lost(error),
            }
        } else {
            // Woken for a control: no read this time.
            drop(guard);
        }
    };
    end_all(&mut attached, &cause);
    refuse_until_stopped(controls);
}

/// Dispatches every attached queue; why the connection ends, if it does.
fn dispatch_all(attached: &mut [Attached]) -> Option<EndCause> {
    for attachment in attached.iter_mut() {
        match panic::catch_unwind(AssertUnwindSafe(|| attachment.live.dispatch())) {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => return Some(EndCause::ConnectionLost(Arc::new(error))),
            Err(_) => return Some(EndCause::HandlerPanicked),
        }
    }
    None
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

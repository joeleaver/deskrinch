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
use wayland_client::{Connection, DispatchError};

use crate::globals::Registry;
use crate::lifecycle::EndCause;
use crate::states::{Attached, Counts, States};

/// What other threads ask of the client's thread.
pub(crate) enum Control {
    /// Run an attachment's setup and, if it succeeds, dispatch its queue.
    Attach(Job),
    /// Detach attachment `id`: it releases what it bound, and is dispatched
    /// as a releasing state until nothing can reach it; acknowledge, when
    /// asked, once it has released.
    Detach(u64, Option<SyncSender<()>>),
    /// The last handle to the client is gone: end the thread.
    Stop,
}

/// An attachment's setup, run on the client's thread with the connection
/// and its globals (or with `None` once the connection has ended, to
/// refuse). A setup that fails, or a refusal, answers its waiter itself; one
/// that succeeds returns what to dispatch and the answer, which the thread
/// gives once the state is attached and counted.
pub(crate) type Job =
    Box<dyn FnOnce(Option<(&Connection, &GlobalList)>) -> Option<(Attached, Answer)> + Send>;

/// Tells an attach's waiter that its state is attached.
pub(crate) type Answer = Box<dyn FnOnce() + Send>;

/// The thread's body.
///
/// Its life is decided by the handles alone: when the last [`Client`] and
/// [`Attachment`] are gone it returns at once, releasing states and all,
/// and the connection closes. A releasing state never keeps it.
///
/// [`Client`]: crate::Client
/// [`Attachment`]: crate::Attachment
pub(crate) fn run(
    connection: &Connection,
    mut registry: Registry,
    counts: Arc<Counts>,
    controls: &Receiver<Control>,
    wake: &OwnedFd,
) {
    let mut states = States::new(counts);
    let cause = loop {
        match take_controls(connection, &mut registry, controls, &mut states) {
            Next::Go => {}
            Next::End(cause) => break cause,
            // The last handle is gone, and with it every attachment.
            Next::Stop => return,
        }
        // A detach's requests leave in the flush below, in this round.
        if let Some(cause) = states.dispatch(&mut registry) {
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
    states.end(&cause);
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
/// state releases what it bound before anything else is dispatched. A
/// setup, a release or a state's drop that panics ends the connection.
///
/// Before a setup, everything read so far is dispatched to the states
/// already attached, the registry's events included: the new state's
/// setup lists the globals as they are then, and hears of those read from
/// then on.
fn take_controls(
    connection: &Connection,
    registry: &mut Registry,
    controls: &Receiver<Control>,
    states: &mut States,
) -> Next {
    while let Ok(control) = controls.try_recv() {
        match control {
            Control::Attach(job) => {
                if let Some(cause) = states.dispatch(registry) {
                    return Next::End(cause);
                }
                let globals = registry.globals();
                match panic::catch_unwind(AssertUnwindSafe(|| job(Some((connection, globals))))) {
                    Ok(Some((new, answer))) => {
                        // Counted before `attach` returns.
                        states.attach(new);
                        answer();
                    }
                    Ok(None) => {}
                    // A setup's handler panicked (its waiter has heard the
                    // end already: the answer was dropped unsent).
                    Err(_) => return Next::End(EndCause::HandlerPanicked),
                }
            }
            Control::Detach(id, acknowledge) => {
                let detached = states.detach(id);
                if let Some(acknowledge) = acknowledge {
                    // The waiter may be gone; nothing else to tell.
                    let _ = acknowledge.send(());
                }
                if detached.is_err() {
                    return Next::End(EndCause::HandlerPanicked);
                }
            }
            Control::Stop => return Next::Stop,
        }
    }
    Next::Go
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

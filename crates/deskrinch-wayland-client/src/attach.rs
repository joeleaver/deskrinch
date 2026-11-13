//! Attaching a crate's state to a [`Client`], and detaching it.

use std::io::ErrorKind;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use wayland_client::backend::WaylandError;
use wayland_client::globals::GlobalList;
use wayland_client::{Connection, DispatchError, EventQueue, QueueHandle};

use crate::client::Client;
use crate::globals::Globals;
use crate::lifecycle::Lifecycle;
use crate::states::{Attached, Queue};
use crate::thread::{Answer, Control};

/// Why a state could not be attached.
#[derive(Debug, thiserror::Error)]
pub enum AttachError<E> {
    /// The crate's own setup failed.
    #[error("the setup failed")]
    Setup(#[source] E),
    /// The connection has ended (it was lost, or a handler on it panicked)
    /// before the state could be attached.
    #[error("the connection has ended")]
    Ended,
    /// Attaching was asked from the client's own thread (from inside a
    /// handler), which would wait for itself.
    #[error("cannot attach from the client's own thread")]
    OnClientThread,
}

/// Set once the attachment has been dropped. A state handed it at setup
/// checks it before telling its subscriber anything: when the attachment
/// is dropped from inside one of that state's own handlers, the events of
/// the batch already read may still reach the state's handlers, and the
/// subscriber must hear none of them.
#[derive(Debug, Clone, Default)]
pub struct Detached(Arc<AtomicBool>);

impl Detached {
    /// Whether the attachment has been dropped.
    #[must_use]
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn set(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// What a crate's setup has, on the client's thread: the globals, from the
/// connection's one registry; its queue's handle; and exchanges with the
/// compositor to load its first state.
#[derive(Debug)]
pub struct Setup<'a, State> {
    globals: &'a GlobalList,
    queue: &'a mut EventQueue<State>,
    detached: Detached,
}

impl<State: 'static> Setup<'_, State> {
    /// The compositor's globals, from the connection's one registry: those
    /// there when the setup began, and any its exchanges bring. What comes
    /// and goes after the setup began reaches the state through
    /// [`Globals`] too. Binding from it ([`GlobalList::bind`], or its
    /// [`registry`](GlobalList::registry) with the state's queue's
    /// [`handle`](Self::handle)) puts the bound object on the state's own
    /// queue.
    #[must_use]
    pub fn globals(&self) -> &GlobalList {
        self.globals
    }

    /// The handle of the state's queue, for binding globals and making
    /// objects whose events go to the state.
    #[must_use]
    pub fn handle(&self) -> QueueHandle<State> {
        self.queue.handle()
    }

    /// The attachment's [`Detached`] flag, for the state to keep.
    #[must_use]
    pub fn detached(&self) -> Detached {
        self.detached.clone()
    }

    /// One exchange with the compositor: everything sent so far is
    /// answered, and the answers are dispatched into `state`. Safe here
    /// because this runs on the client's thread, the connection's only
    /// reader.
    ///
    /// # Errors
    ///
    /// When the connection fails.
    pub fn roundtrip(&mut self, state: &mut State) -> Result<usize, DispatchError> {
        self.queue.roundtrip(state)
    }
}

/// A state attached to a [`Client`]: dispatched on the client's thread
/// until this is dropped.
///
/// Dropping it detaches the state: its [`Detached`] flag is set at once, so
/// its subscriber hears nothing more, and the client's thread calls its
/// [`Lifecycle::release`], whose requests leave in that same round. The
/// drop returns once the state has released. The thread then keeps
/// dispatching it, its events consumed and dropped, until
/// [`Lifecycle::released`] says nothing can reach it, and drops it then
/// (see [`Client::states`]). Dropped on the client's thread itself (from
/// inside a handler), it does not wait, which would never end: the state is
/// released as soon as the thread is done with the batch of events it is
/// dispatching.
#[derive(Debug)]
pub struct Attachment {
    client: Client,
    id: u64,
    detached: Detached,
}

/// What became of the requests a flush was asked to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flushed {
    /// They reached the compositor's socket.
    Sent,
    /// The socket would not take them all now (the compositor is slow to
    /// read): the client's thread sends the rest once it can. Nothing is
    /// lost and nothing has failed.
    Pending,
}

impl Attachment {
    /// Sends the requests the crate has made. A socket that would block is
    /// not a failure: the client's thread finishes the flush
    /// ([`Flushed::Pending`]).
    ///
    /// # Errors
    ///
    /// When the connection has failed.
    pub fn flush(&self) -> Result<Flushed, WaylandError> {
        let shared = &self.client.shared;
        match shared.connection.flush() {
            Ok(()) => Ok(Flushed::Sent),
            Err(WaylandError::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                shared.wake();
                Ok(Flushed::Pending)
            }
            Err(error) => Err(error),
        }
    }
}

impl Drop for Attachment {
    fn drop(&mut self) {
        self.detached.set();
        let shared = &self.client.shared;
        if shared.on_thread() {
            shared.send(Control::Detach(self.id, None));
            return;
        }
        let (acknowledge, acknowledged) = mpsc::sync_channel(1);
        shared.send(Control::Detach(self.id, Some(acknowledge)));
        // Answered once the state has released, or refused when the thread
        // has ended: either way its subscriber hears nothing more.
        let _ = acknowledged.recv();
    }
}

impl Client {
    /// Attaches a state to the connection: on the client's thread, a new
    /// queue is made, `setup` builds the state from the connection's
    /// globals (binding what it needs, and exchanging with the compositor
    /// through [`Setup::roundtrip`] to load its first state), and the queue
    /// is dispatched into the state from then on. Returns once that is done,
    /// with what `setup` returned beside the state.
    ///
    /// The wait has no time limit: it ends when the setup has run, or when
    /// the connection ends.
    ///
    /// # Errors
    ///
    /// [`AttachError::Setup`] with the setup's own error,
    /// [`AttachError::Ended`] when the connection has ended, and
    /// [`AttachError::OnClientThread`] when called from a handler.
    pub fn attach<State, Out, E>(
        &self,
        setup: impl FnOnce(&mut Setup<'_, State>) -> Result<(State, Out), E> + Send + 'static,
    ) -> Result<(Attachment, Out), AttachError<E>>
    where
        State: Lifecycle + Globals + Send + 'static,
        Out: Send + 'static,
        E: Send + 'static,
    {
        let shared = &self.shared;
        if shared.on_thread() {
            return Err(AttachError::OnClientThread);
        }
        let id = shared.next_id();
        let detached = Detached::default();
        let (answer, answered) = mpsc::sync_channel(1);
        let job_detached = detached.clone();
        shared.send(Control::Attach(Box::new(
            move |connection: Option<(&Connection, &GlobalList)>| {
                let Some((connection, globals)) = connection else {
                    drop(answer.send(Err(AttachError::Ended)));
                    return None;
                };
                match set_up(connection, globals, job_detached, setup) {
                    Ok((queue, state, out)) => {
                        let attached = Attached {
                            id,
                            live: Box::new(Queue { queue, state }),
                        };
                        // The waiter may be gone; nothing else to tell.
                        let answered: Answer = Box::new(move || drop(answer.send(Ok(out))));
                        Some((attached, answered))
                    }
                    Err(error) => {
                        drop(answer.send(Err(error)));
                        None
                    }
                }
            },
        )));
        let out = answered.recv().map_err(|_| AttachError::Ended)??;
        Ok((
            Attachment {
                client: self.clone(),
                id,
                detached,
            },
            out,
        ))
    }
}

/// Runs `setup` on a new queue, with the connection's globals.
fn set_up<State, Out, E>(
    connection: &Connection,
    globals: &GlobalList,
    detached: Detached,
    setup: impl FnOnce(&mut Setup<'_, State>) -> Result<(State, Out), E>,
) -> Result<(EventQueue<State>, State, Out), AttachError<E>>
where
    State: 'static,
{
    let mut queue = connection.new_event_queue();
    let (state, out) = setup(&mut Setup {
        globals,
        queue: &mut queue,
        detached,
    })
    .map_err(AttachError::Setup)?;
    Ok((queue, state, out))
}

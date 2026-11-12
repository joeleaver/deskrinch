//! Attaching a crate's state to a [`Client`], and detaching it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use wayland_client::globals::{GlobalError, GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, Dispatch, DispatchError, EventQueue, QueueHandle};

use crate::client::Client;
use crate::ended::Ended;
use crate::thread::{Attached, Control, Queue};

/// Why a state could not be attached.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AttachError<E> {
    /// The compositor's globals could not be listed.
    #[error("could not list the compositor's globals")]
    Globals(#[source] GlobalError),
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

/// What a crate's setup has, on the client's thread: the globals, its
/// queue's handle, and exchanges with the compositor to load its first
/// state.
#[derive(Debug)]
pub struct Setup<'a, State> {
    globals: &'a GlobalList,
    queue: &'a mut EventQueue<State>,
    detached: Detached,
}

impl<State: 'static> Setup<'_, State> {
    /// The compositor's globals, as listed at setup; later ones arrive as
    /// `wl_registry` events on the state.
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
/// Dropping it returns once the client's thread will run none of the
/// state's handlers again. Dropped on the client's thread itself (from
/// inside a handler), it does not wait, which would never end: the state is
/// dropped as soon as the thread is done with the batch of events it is
/// dispatching, and its [`Detached`] flag is set at once, so its subscriber
/// hears nothing more.
#[derive(Debug)]
pub struct Attachment {
    client: Client,
    id: u64,
    detached: Detached,
}

impl Attachment {
    /// The connection, for flushing the requests the crate sends.
    #[must_use]
    pub fn connection(&self) -> &Connection {
        self.client.connection()
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
        // Answered once the state is dropped, or refused when the thread
        // has ended: either way it runs no handler of the state again.
        let _ = acknowledged.recv();
    }
}

impl Client {
    /// Attaches a state to the connection: on the client's thread, a new
    /// queue and its registry are made, `setup` builds the state from the
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
    /// [`AttachError::Globals`] when the globals cannot be listed,
    /// [`AttachError::Ended`] when the connection has ended, and
    /// [`AttachError::OnClientThread`] when called from a handler.
    pub fn attach<State, Out, E>(
        &self,
        setup: impl FnOnce(&mut Setup<'_, State>) -> Result<(State, Out), E> + Send + 'static,
    ) -> Result<(Attachment, Out), AttachError<E>>
    where
        State: Ended + Dispatch<WlRegistry, GlobalListContents> + Send + 'static,
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
            move |connection: Option<&Connection>| {
                let Some(connection) = connection else {
                    drop(answer.send(Err(AttachError::Ended)));
                    return None;
                };
                match set_up(connection, job_detached, setup) {
                    Ok((queue, state, out)) => {
                        drop(answer.send(Ok(out)));
                        Some(Attached {
                            id,
                            live: Box::new(Queue { queue, state }),
                        })
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

/// Lists the globals on a new queue and runs `setup` on it.
fn set_up<State, Out, E>(
    connection: &Connection,
    detached: Detached,
    setup: impl FnOnce(&mut Setup<'_, State>) -> Result<(State, Out), E>,
) -> Result<(EventQueue<State>, State, Out), AttachError<E>>
where
    State: Dispatch<WlRegistry, GlobalListContents> + 'static,
{
    let (globals, mut queue) =
        registry_queue_init::<State>(connection).map_err(AttachError::Globals)?;
    let (state, out) = setup(&mut Setup {
        globals: &globals,
        queue: &mut queue,
        detached,
    })
    .map_err(AttachError::Setup)?;
    Ok((queue, state, out))
}

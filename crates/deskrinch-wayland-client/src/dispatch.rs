//! [`DispatchThread`]: one event queue, dispatched on a thread it owns.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::{Connection, Dispatch, DispatchError, EventQueue, QueueHandle};

use crate::Client;

/// What a dispatch thread's state tells the thread about its end.
pub trait Ended {
    /// The connection failed while the thread dispatched: the state ends
    /// and tells its subscriber why. Not called when the thread was told to
    /// stop.
    fn connection_lost(&mut self, error: DispatchError);

    /// Whether the state has ended (its protocol said it is finished, or
    /// the connection was lost): the thread then stops.
    fn ended(&self) -> bool;
}

/// The user data of the `wl_display.sync` a [`DispatchThread`] sends to
/// wake its thread when dropped. A dispatch state handles it with
/// `wayland_client::delegate_dispatch!(State: [WlCallback: Wake] => Wake);`.
#[derive(Debug, Clone, Copy)]
pub struct Wake;

impl<State> Dispatch<WlCallback, Wake, State> for Wake
where
    State: Dispatch<WlCallback, Wake>,
{
    fn event(
        _: &mut State,
        _: &WlCallback,
        _: wl_callback::Event,
        _: &Wake,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

/// One event queue on a [`Client`]'s connection, dispatched on a thread of
/// its own until this is dropped or the connection fails.
///
/// Dropping it stops the thread: it wakes the thread with a
/// `wl_display.sync` and waits for it, unless dropped on that thread (from
/// inside the state's own callbacks).
#[derive(Debug)]
pub struct DispatchThread<State: Dispatch<WlCallback, Wake> + 'static> {
    connection: Connection,
    queue: QueueHandle<State>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl<State> DispatchThread<State>
where
    State: Ended + Dispatch<WlCallback, Wake> + Send + 'static,
{
    /// Dispatches `queue` into `state` on a thread named `name`.
    ///
    /// # Errors
    ///
    /// When the thread cannot be started.
    pub fn spawn(
        name: &str,
        client: &Client,
        queue: EventQueue<State>,
        state: State,
    ) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = queue.handle();
        let thread = thread::Builder::new().name(name.to_owned()).spawn({
            let stop = Arc::clone(&stop);
            move || dispatch(queue, state, &stop)
        })?;
        Ok(Self {
            connection: client.connection().clone(),
            queue: handle,
            stop,
            thread: Some(thread),
        })
    }
}

impl<State: Dispatch<WlCallback, Wake> + 'static> DispatchThread<State> {
    /// The connection the queue is on, for sending requests and flushing.
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}

/// The thread: reads and dispatches events until told to stop, the state
/// ends, or the connection fails.
fn dispatch<State: Ended>(mut queue: EventQueue<State>, mut state: State, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        if let Err(error) = queue.blocking_dispatch(&mut state) {
            if !stop.load(Ordering::Acquire) {
                state.connection_lost(error);
            }
            return;
        }
        if state.ended() {
            return;
        }
    }
}

impl<State: Dispatch<WlCallback, Wake> + 'static> Drop for DispatchThread<State> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake the dispatch thread: the compositor answers the sync, the
        // thread dispatches the answer and sees `stop`. If the connection is
        // already gone the thread has ended or is ending on the error.
        self.connection.display().sync(&self.queue, Wake);
        // A failed flush means the connection is gone, which ends the thread
        // as well; a drop has nothing else to do with the error.
        drop(self.connection.flush());
        let Some(thread) = self.thread.take() else {
            return;
        };
        if thread.thread().id() != thread::current().id() {
            // The thread can only panic in the state's callbacks, and that
            // panic has already been reported on the thread; re-raising it
            // from a drop would abort.
            drop(thread.join());
        }
    }
}

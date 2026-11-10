//! The [`WindowManagement`] trait and the handle that implements it over a
//! Wayland connection.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use wayland_client::{Connection, EventQueue, QueueHandle};

use crate::backend::{self, Connected, Shared, State};
use crate::capabilities::{Action, Capabilities};
use crate::error::{ActionError, ConnectError, EndReason};
use crate::window::{Event, Window, WindowId};

/// The compositor's window list and the actions on it, whatever protocol
/// carries them.
///
/// [`WindowManager`] implements it over Wayland; a caller written against
/// the trait can be tested with an implementation of its own.
pub trait WindowManagement: Send + Sync {
    /// What this connection reports and which actions it supports.
    fn capabilities(&self) -> Capabilities;

    /// The windows as last reported, in the order they appeared. Empty once
    /// the connection has ended.
    fn windows(&self) -> Vec<Window>;

    /// Requests `action` on the window `id`. Success means the request was
    /// sent; the compositor decides, and what it does arrives as an
    /// [`Event`].
    ///
    /// # Errors
    ///
    /// [`ActionError::Unsupported`] when the connection lacks the action
    /// (see [`Capabilities::supports`]), [`ActionError::UnknownWindow`] for a
    /// window not in the list, and others when the connection cannot carry
    /// the request.
    fn perform(&self, id: WindowId, action: Action) -> Result<(), ActionError>;
}

/// A connection to the compositor's window management.
///
/// It owns a Wayland connection of its own, dispatched on a thread it owns,
/// and calls the subscriber given to [`connect`] on that thread. Methods may
/// be called from any thread. Dropping it stops the thread (waiting for it,
/// unless dropped on that thread, from inside the subscriber) and closes the
/// connection.
#[derive(Debug)]
pub struct WindowManager {
    connection: Connection,
    queue: QueueHandle<State>,
    shared: Arc<Shared>,
    capabilities: Capabilities,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// Connects to the compositor named by the environment (`WAYLAND_DISPLAY` or
/// `WAYLAND_SOCKET`), picks the protocol it offers, and loads the window
/// list.
///
/// `subscriber` hears an [`Event::Added`] for every window already open
/// before this returns, called on this thread, and every later change on the
/// dispatch thread. It must not block for long: while it runs, no further
/// events are read.
///
/// # Errors
///
/// [`ConnectError::Unsupported`] or [`ConnectError::NotGranted`] when the
/// compositor offers no usable protocol, and others when the connection or
/// its first exchanges fail.
pub fn connect(subscriber: impl Fn(Event) + Send + 'static) -> Result<WindowManager, ConnectError> {
    let connection = Connection::connect_to_env()?;
    let Connected {
        connection,
        queue,
        state,
        shared,
        capabilities,
    } = backend::connect(connection, Box::new(subscriber))?;
    let stop = Arc::new(AtomicBool::new(false));
    let queue_handle = queue.handle();
    let thread = thread::Builder::new()
        .name("deskrinch-window-management".into())
        .spawn({
            let stop = Arc::clone(&stop);
            move || dispatch(queue, state, &stop)
        })
        .map_err(ConnectError::Thread)?;
    Ok(WindowManager {
        connection,
        queue: queue_handle,
        shared,
        capabilities,
        stop,
        thread: Some(thread),
    })
}

/// The dispatch thread: reads and dispatches events until told to stop or the
/// connection fails.
fn dispatch(mut queue: EventQueue<State>, mut state: State, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        if let Err(error) = queue.blocking_dispatch(&mut state) {
            if !stop.load(Ordering::Acquire) {
                state.end(EndReason::ConnectionLost(Arc::new(error)));
            }
            return;
        }
        if state.ended() {
            return;
        }
    }
}

impl WindowManagement for WindowManager {
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn windows(&self) -> Vec<Window> {
        self.shared.lock().list.snapshot()
    }

    fn perform(&self, id: WindowId, action: Action) -> Result<(), ActionError> {
        self.shared.lock().request(self.capabilities, id, action)?;
        self.connection.flush().map_err(ActionError::Send)
    }
}

impl Drop for WindowManager {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake the dispatch thread: the compositor answers the sync, the
        // thread dispatches the answer and sees `stop`. If the connection is
        // already gone the thread has ended or is ending on the error.
        self.connection.display().sync(&self.queue, ());
        // A failed flush means the connection is gone, which ends the thread
        // as well; a drop has nothing else to do with the error.
        drop(self.connection.flush());
        let Some(thread) = self.thread.take() else {
            return;
        };
        if thread.thread().id() != thread::current().id() {
            // The thread can only panic in the subscriber, and that panic
            // has already been reported on the thread; re-raising it from a
            // drop would abort.
            drop(thread.join());
        }
    }
}

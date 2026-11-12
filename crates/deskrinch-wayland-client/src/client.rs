//! [`Client`]: one connection to the compositor, read and dispatched by one
//! thread, shareable between crates.

use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle, ThreadId};

use rustix::event::{EventfdFlags, eventfd};
use wayland_client::protocol::wl_display::WlDisplay;
use wayland_client::{ConnectError, Connection};

use crate::thread::{self as client_thread, Control};

/// The client thread's name, as `/proc` shows it.
pub(crate) const THREAD_NAME: &str = "deskrinch-wl";

/// Why a [`Client`] could not be made.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// No Wayland compositor could be reached (`WAYLAND_DISPLAY` or
    /// `WAYLAND_SOCKET`).
    #[error("could not connect to the Wayland compositor")]
    Connect(#[from] ConnectError),
    /// The client's thread, or what wakes it, could not be made.
    #[error("could not start the client's thread")]
    Thread(#[source] io::Error),
}

/// A connection to the compositor that several crates share, read and
/// dispatched by one thread of its own.
///
/// Each crate attaches a state to it ([`Client::attach`]); the client's
/// thread is the connection's only reader, and dispatches every attached
/// state's queue. Cloning it clones the handle, not the connection. The
/// connection and its thread end when the last clone and the last
/// [`Attachment`](crate::Attachment) are gone; everything on the connection
/// shares its fate (see [`EndCause`](crate::EndCause)).
#[derive(Debug, Clone)]
pub struct Client {
    pub(crate) shared: Arc<Shared>,
}

/// What every handle to a client shares.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) connection: Connection,
    controls: Mutex<Sender<Control>>,
    wake: OwnedFd,
    thread: Mutex<Option<JoinHandle<()>>>,
    thread_id: ThreadId,
    next_id: AtomicU64,
}

impl Client {
    /// Connects to the compositor named by the environment
    /// (`WAYLAND_DISPLAY` or `WAYLAND_SOCKET`) and starts the client's
    /// thread.
    ///
    /// # Errors
    ///
    /// When no compositor can be reached, or the thread cannot be started.
    pub fn connect() -> Result<Self, ClientError> {
        Self::from_connection(Connection::connect_to_env()?)
    }

    /// A client on an existing connection, which nothing else may read: its
    /// thread is the only reader.
    ///
    /// # Errors
    ///
    /// When the client's thread cannot be started.
    pub fn from_connection(connection: Connection) -> Result<Self, ClientError> {
        let wake = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
            .map_err(|error| ClientError::Thread(error.into()))?;
        let thread_wake = wake.try_clone().map_err(ClientError::Thread)?;
        let (controls, received) = mpsc::channel();
        let thread_connection = connection.clone();
        let thread = thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || client_thread::run(&thread_connection, &received, &thread_wake))
            .map_err(ClientError::Thread)?;
        let thread_id = thread.thread().id();
        Ok(Self {
            shared: Arc::new(Shared {
                connection,
                controls: Mutex::new(controls),
                wake,
                thread: Mutex::new(Some(thread)),
                thread_id,
                next_id: AtomicU64::new(0),
            }),
        })
    }

    /// The connection's `wl_display`, for a `sync` on a state's queue. The
    /// connection itself is not handed out: reading it is the client's
    /// thread's alone, and an attachment flushes through
    /// [`Attachment::flush`](crate::Attachment::flush).
    #[must_use]
    pub fn display(&self) -> WlDisplay {
        self.shared.connection.display()
    }
}

impl Shared {
    /// Asks the client's thread for `control`, and wakes it. A thread that
    /// has ended answers nothing more; the caller's answer channel then
    /// reports it.
    pub(crate) fn send(&self, control: Control) {
        let controls = self.controls.lock().unwrap_or_else(PoisonError::into_inner);
        // A failed send means the thread is gone: the caller's own answer
        // channel says so.
        drop(controls.send(control));
        drop(controls);
        self.wake();
    }

    /// Wakes the client's thread (to take a control, or to finish a flush).
    pub(crate) fn wake(&self) {
        // Non-blocking: a full counter means the thread is being woken
        // already.
        let _ = rustix::io::write(self.wake.as_fd(), &1_u64.to_ne_bytes());
    }

    /// Whether this is the client's thread.
    pub(crate) fn on_thread(&self) -> bool {
        thread::current().id() == self.thread_id
    }

    /// A new attachment's id.
    pub(crate) fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        self.send(Control::Stop);
        let thread = self
            .thread
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(thread) = thread
            && !self.on_thread()
        {
            // The thread catches its handlers' panics; a panic here would be
            // its own, already reported, and a drop has nothing to add.
            drop(thread.join());
        }
    }
}

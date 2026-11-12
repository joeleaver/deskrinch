//! The [`WindowManagement`] trait and the handle that implements it over a
//! Wayland connection.

use std::sync::Arc;

use deskrinch_wayland_client::{Client, DispatchThread};

use crate::backend::{self, Connected, Shared, State};
use crate::capabilities::{Action, Capabilities};
use crate::error::{ActionError, ConnectError};
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
/// It has an event queue of its own, dispatched on a thread it owns, and
/// calls the subscriber given to [`connect`] or [`connect_with`] on that
/// thread. Methods may be called from any thread. Dropping it stops the
/// thread (waiting for it, unless dropped on that thread, from inside the
/// subscriber); the connection closes once nothing else uses it.
#[derive(Debug)]
pub struct WindowManager {
    dispatch: DispatchThread<State>,
    shared: Arc<Shared>,
    capabilities: Capabilities,
}

/// Connects to the compositor named by the environment (`WAYLAND_DISPLAY` or
/// `WAYLAND_SOCKET`) on a connection of its own, picks the protocol it
/// offers, and loads the window list: [`connect_with`] on a new
/// [`Client`].
///
/// # Errors
///
/// As [`connect_with`], and [`ConnectError::Connect`] when no compositor can
/// be reached.
pub fn connect(subscriber: impl Fn(Event) + Send + 'static) -> Result<WindowManager, ConnectError> {
    connect_with(&Client::connect()?, subscriber)
}

/// Picks the protocol `client`'s compositor offers and loads the window
/// list, on an event queue and dispatch thread of its own on `client`'s
/// connection, which other crates (`deskrinch-workspaces`) may share.
///
/// A shared connection is shared in full: a protocol error raised by any of
/// its users, or the compositor going away, ends this connection's stream
/// too ([`Event::Ended`]).
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
pub fn connect_with(
    client: &Client,
    subscriber: impl Fn(Event) + Send + 'static,
) -> Result<WindowManager, ConnectError> {
    let Connected {
        queue,
        state,
        shared,
        capabilities,
    } = backend::connect(client, Box::new(subscriber))?;
    let dispatch = DispatchThread::spawn("deskrinch-window-management", client, queue, state)
        .map_err(ConnectError::Thread)?;
    Ok(WindowManager {
        dispatch,
        shared,
        capabilities,
    })
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
        self.dispatch
            .connection()
            .flush()
            .map_err(ActionError::Send)
    }
}

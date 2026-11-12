//! The [`WindowManagement`] trait and the handle that implements it over a
//! Wayland connection.

use std::sync::Arc;

use deskrinch_wayland_client::{Attachment, Client, ClientError};

use crate::backend::{self, Connected, Shared};
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
/// Its state is attached to a [`Client`], whose one thread dispatches it and
/// calls the subscriber given to [`connect`] or [`connect_with`]. Methods
/// may be called from any thread. Dropping it detaches the state: it returns
/// once the client's thread will call the subscriber no more (dropped from
/// inside the subscriber, it returns at once, and the subscriber hears
/// nothing more). The connection closes once nothing else uses it.
#[derive(Debug)]
pub struct WindowManager {
    attachment: Attachment,
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
    let client = Client::connect().map_err(|error| match error {
        ClientError::Connect(error) => ConnectError::Connect(error),
        ClientError::Thread(error) => ConnectError::Thread(error),
    })?;
    connect_with(&client, subscriber)
}

/// Picks the protocol `client`'s compositor offers and loads the window
/// list, attached to `client`'s connection, which other crates
/// (`deskrinch-workspaces`) may share; the client's one thread dispatches
/// them all.
///
/// A shared connection is shared in full: a protocol error raised by any of
/// its users, the compositor going away, or a panic in any subscriber on it
/// ends this connection's stream too ([`Event::Ended`]).
///
/// `subscriber` is called on the client's thread: an [`Event::Added`] for
/// every window already open before this returns, then every later change.
/// It must not block for long: while it runs, nothing else on the
/// connection is dispatched.
///
/// # Errors
///
/// [`ConnectError::Unsupported`] or [`ConnectError::NotGranted`] when the
/// compositor offers no usable protocol, [`ConnectError::OnClientThread`]
/// when called from inside a subscriber on `client`'s thread, and others
/// when the connection or its first exchanges fail.
pub fn connect_with(
    client: &Client,
    subscriber: impl Fn(Event) + Send + 'static,
) -> Result<WindowManager, ConnectError> {
    let Connected {
        attachment,
        shared,
        capabilities,
    } = backend::connect(client, Box::new(subscriber))?;
    Ok(WindowManager {
        attachment,
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
        // A socket that would block is not a failure: the client's thread
        // finishes sending.
        self.attachment
            .flush()
            .map(|_flushed| ())
            .map_err(ActionError::Send)
    }
}

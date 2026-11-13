//! The [`Workspaces`] trait and the handle that implements it over a
//! Wayland connection.

use std::sync::Arc;

use deskrinch_wayland_client::{Attachment, Client, ClientError};

use crate::backend::{self, Connected, Shared};
use crate::capabilities::{Action, Capabilities};
use crate::error::{ActionError, ConnectError};
use crate::workspace::{Event, Group, Workspace, WorkspaceId};

/// The compositor's workspaces and the actions on them, whatever protocol
/// carries them.
///
/// [`WorkspaceManager`] implements it over Wayland; a caller written against
/// the trait can be tested with an implementation of its own.
pub trait Workspaces: Send + Sync {
    /// What this connection reports and which actions it can carry.
    fn capabilities(&self) -> Capabilities;

    /// The workspaces as last reported, in the compositor's order. Empty
    /// once the connection has ended.
    fn workspaces(&self) -> Vec<Workspace>;

    /// The groups as last reported.
    fn groups(&self) -> Vec<Group>;

    /// Requests `action` on workspace `id`. Success means the request was
    /// sent; the compositor decides, and what it does arrives as an
    /// [`Event`].
    ///
    /// # Errors
    ///
    /// [`ActionError::Unsupported`] when the protocol cannot carry the
    /// action, [`ActionError::NotAllowed`] when the compositor does not allow
    /// it on that workspace, [`ActionError::UnknownWorkspace`] for one not
    /// listed, and others when the connection cannot carry the request.
    fn perform(&self, id: WorkspaceId, action: Action) -> Result<(), ActionError>;
}

/// A connection to the compositor's workspaces.
///
/// Its state is attached to a [`Client`], whose one thread dispatches it and
/// calls the subscriber given to [`connect`] or [`connect_with`]. Methods
/// may be called from any thread. Dropping it detaches the state: it returns
/// once the client's thread will call the subscriber no more (dropped from
/// inside the subscriber, it returns at once, and the subscriber hears
/// nothing more). The connection closes once nothing else uses it.
#[derive(Debug)]
pub struct WorkspaceManager {
    attachment: Attachment,
    shared: Arc<Shared>,
    capabilities: Capabilities,
}

/// Connects to the compositor named by the environment (`WAYLAND_DISPLAY` or
/// `WAYLAND_SOCKET`) on a connection of its own, picks the protocol it
/// offers, and loads the workspaces: [`connect_with`] on a new [`Client`].
///
/// # Errors
///
/// As [`connect_with`], and [`ConnectError::Connect`] when no compositor can
/// be reached.
pub fn connect(
    subscriber: impl Fn(Event) + Send + 'static,
) -> Result<WorkspaceManager, ConnectError> {
    let client = Client::connect().map_err(|error| match error {
        ClientError::Connect(error) => ConnectError::Connect(error),
        ClientError::Thread(error) => ConnectError::Thread(error),
    })?;
    connect_with(&client, subscriber)
}

/// Picks the protocol `client`'s compositor offers and loads the
/// workspaces, attached to `client`'s connection, which other crates
/// (`deskrinch-window-management`) may share; the client's one thread
/// dispatches them all.
///
/// A shared connection is shared in full: a protocol error raised by any of
/// its users, the compositor going away, or a panic in any subscriber on it
/// ends this connection's stream too ([`Event::Ended`]).
///
/// `subscriber` is called on the client's thread: an
/// [`Event::GroupChanged`] for every group and an [`Event::Added`] for every
/// workspace already there before this returns, then every later change. It
/// must not block for long: while it runs, nothing else on the connection is
/// dispatched.
///
/// # Errors
///
/// [`ConnectError::Unsupported`] when the compositor offers no protocol this
/// crate speaks, [`ConnectError::OnClientThread`] when called from inside a
/// subscriber on `client`'s thread, and others when the connection or its
/// first exchanges fail.
pub fn connect_with(
    client: &Client,
    subscriber: impl Fn(Event) + Send + 'static,
) -> Result<WorkspaceManager, ConnectError> {
    let Connected {
        attachment,
        shared,
        capabilities,
    } = backend::connect(client, Box::new(subscriber))?;
    Ok(WorkspaceManager {
        attachment,
        shared,
        capabilities,
    })
}

impl Workspaces for WorkspaceManager {
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn workspaces(&self) -> Vec<Workspace> {
        self.shared.lock().list.snapshot()
    }

    fn groups(&self) -> Vec<Group> {
        self.shared.lock().list.groups()
    }

    fn perform(&self, id: WorkspaceId, action: Action) -> Result<(), ActionError> {
        self.shared.lock().request(self.capabilities, id, action)?;
        // A socket that would block is not a failure: the client's thread
        // finishes sending.
        self.attachment
            .flush()
            .map(|_flushed| ())
            .map_err(ActionError::Send)
    }
}

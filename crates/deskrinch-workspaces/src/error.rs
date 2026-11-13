//! Errors, as typed values.

use std::io;
use std::sync::Arc;

use wayland_client::globals::{BindError, GlobalError};
use wayland_client::{ConnectError as WaylandConnectError, DispatchError};

use crate::capabilities::Action;
use crate::workspace::WorkspaceId;

/// Why [`connect`](crate::connect) or [`connect_with`](crate::connect_with)
/// failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConnectError {
    /// No Wayland compositor could be reached (`WAYLAND_DISPLAY` or
    /// `WAYLAND_SOCKET`).
    #[error("could not connect to the Wayland compositor")]
    Connect(#[from] WaylandConnectError),
    /// The compositor's globals could not be listed.
    #[error("could not list the compositor's globals")]
    Globals(#[from] GlobalError),
    /// The compositor offers none of the workspace protocols this crate
    /// speaks (sway, at 1.11, offers its workspaces through its IPC only).
    #[error(
        "the compositor offers no workspace protocol this crate speaks ({})",
        crate::backend::SUPPORTED_GLOBALS.join(", ")
    )]
    Unsupported,
    /// The compositor offers the protocol at a version older than this crate
    /// needs.
    #[error("the compositor offers {interface} older than version {required}")]
    VersionTooOld {
        /// The protocol's global interface.
        interface: &'static str,
        /// The lowest version this crate binds.
        required: u32,
        /// Why binding failed.
        source: BindError,
    },
    /// The exchange that loads the initial workspaces failed.
    #[error("could not load the initial workspaces")]
    InitialSync(#[source] DispatchError),
    /// The client's thread, which dispatches the connection, could not be
    /// started.
    #[error("could not start the client's thread")]
    Thread(#[source] io::Error),
    /// The shared connection ended (it was lost, or a handler on it
    /// panicked) before the workspaces could be loaded.
    #[error("the connection to the compositor has ended")]
    ConnectionEnded,
    /// [`connect_with`](crate::connect_with) was called from the client's
    /// own thread (from inside a subscriber), which would wait for itself.
    #[error("cannot connect from inside a subscriber on the client's thread")]
    OnClientThread,
}

/// Why an [`Action`] could not be requested.
///
/// Success means the request reached the compositor, which then decides;
/// the outcome arrives as an [`Event`](crate::Event).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ActionError {
    /// The workspace is not (or no longer) listed.
    #[error("{0} is not listed")]
    UnknownWorkspace(WorkspaceId),
    /// This connection's protocol has no such action (see
    /// [`Capabilities::supports`](crate::Capabilities::supports)).
    #[error("the compositor's protocol cannot {0}")]
    Unsupported(Action),
    /// The compositor does not allow the action on this workspace now (see
    /// [`Workspace::actions`](crate::Workspace::actions)).
    #[error("the compositor does not allow {action} on {workspace}")]
    NotAllowed {
        /// The action.
        action: Action,
        /// The workspace.
        workspace: WorkspaceId,
    },
    /// The connection has ended (see [`Event::Ended`](crate::Event::Ended)).
    #[error("the workspace connection has ended")]
    Ended,
    /// The request could not be sent to the compositor.
    #[error("could not send the request to the compositor")]
    Send(#[source] wayland_client::backend::WaylandError),
}

/// Why a connection stopped reporting workspaces.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum EndReason {
    /// The compositor said it will report no more workspaces.
    #[error("the compositor stopped reporting workspaces")]
    Finished,
    /// The connection to the compositor failed.
    #[error("the connection to the compositor was lost")]
    ConnectionLost(#[source] Arc<DispatchError>),
    /// A subscriber or handler of some crate on the same connection
    /// panicked; the panic was reported where it happened, and every stream
    /// on the connection ended.
    #[error("a handler on the connection panicked")]
    HandlerPanicked,
}

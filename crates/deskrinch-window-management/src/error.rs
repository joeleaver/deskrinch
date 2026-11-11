//! Errors, as typed values.

use std::io;
use std::sync::Arc;

use wayland_client::globals::{BindError, GlobalError};
use wayland_client::{ConnectError as WaylandConnectError, DispatchError};

use crate::capabilities::Action;
use crate::window::WindowId;

/// Why [`connect`](crate::connect) failed.
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
    /// The compositor offers none of the window-management protocols this
    /// crate speaks.
    #[error(
        "the compositor offers no window-management protocol this crate speaks \
         ({})",
        crate::backend::SUPPORTED_GLOBALS.join(", ")
    )]
    Unsupported,
    /// The compositor offers none of the protocols, and looks like KWin,
    /// which offers its window-management protocol only to clients it has
    /// granted it to. "Looks like KWin" is inferred: the compositor offers
    /// other `org_kde_kwin_*` globals. The `deskrinch-kwin-access` crate says
    /// how a client is granted [`KWIN_INTERFACE`](crate::KWIN_INTERFACE).
    #[error(
        "the compositor looks like KWin and has not granted this client \
         org_kde_plasma_window_management; see the deskrinch-kwin-access crate"
    )]
    NotGranted,
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
    /// The exchange that loads the initial window list failed.
    #[error("could not load the initial window list")]
    InitialSync(#[source] DispatchError),
    /// The thread that dispatches the connection could not be started.
    #[error("could not start the dispatch thread")]
    Thread(#[source] io::Error),
}

/// Why an [`Action`] could not be requested.
///
/// Success means the request reached the compositor, which then decides; the
/// outcome arrives as an [`Event`](crate::Event).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ActionError {
    /// The window is not (or no longer) in the list.
    #[error("{0} is not in the window list")]
    UnknownWindow(WindowId),
    /// This connection's protocol has no such action (see
    /// [`Capabilities::actions`](crate::Capabilities::actions)).
    #[error("the compositor's protocol cannot {0}")]
    Unsupported(Action),
    /// Activating needs a seat and the compositor offers none.
    #[error("the compositor offers no seat to activate the window on")]
    NoSeat,
    /// The connection has ended (see [`Event::Ended`](crate::Event::Ended)).
    #[error("the window-management connection has ended")]
    Ended,
    /// The request could not be sent to the compositor.
    #[error("could not send the request to the compositor")]
    Send(#[source] wayland_client::backend::WaylandError),
}

/// Why a connection stopped reporting windows.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum EndReason {
    /// The compositor said it will report no more windows.
    #[error("the compositor stopped reporting windows")]
    Finished,
    /// The connection to the compositor failed.
    #[error("the connection to the compositor was lost")]
    ConnectionLost(#[source] Arc<DispatchError>),
}

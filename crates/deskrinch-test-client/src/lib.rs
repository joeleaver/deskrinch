//! Throwaway Wayland clients for deskrinch's compositor tests: windows of
//! known content, and changes to the session's outputs.
//!
//! - [`TestClient`] is a connection of its own, dispatched on its own
//!   thread so that it answers pings and configures like a live
//!   application. Its [`TestWindow`]s are mapped `xdg_toplevel`s that draw
//!   a [`Paint`] (nothing, one colour, or four quarters) at whatever size
//!   the compositor asks for, with a buffer scale of 1.
//! - [`Compositor::current`] says which nested compositor the tests run
//!   in, and refuses to go on outside one.
//! - [`configure_output`] changes an output's scale or transform, under a
//!   compositor that offers `wlr-output-management` (not KWin).
//!
//! What windows the tests look at and act on (the window-management
//! crate's), and what windows they photograph (`deskrinch-test-screen`'s),
//! come from here. Like those tests, it is for nested sessions only
//! (`cargo xtask compositor-tests`, `cargo xtask session`): it opens windows
//! and reconfigures outputs on whatever compositor `WAYLAND_DISPLAY` names.

mod client;
mod compositor;
mod outputs;
mod paint;
mod window;

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

pub use client::TestClient;
pub use compositor::Compositor;
pub use outputs::{OutputChange, OutputSettings, configure_every_output, configure_output};
pub use paint::Paint;
pub use window::TestWindow;

/// How long a window waits to be drawn as asked: generous, since a loaded
/// machine runs nested compositors slowly.
pub const PATIENCE: Duration = Duration::from_secs(60);

/// Why a test client could not do what was asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// There is no compositor to connect to.
    #[error("could not connect to the compositor")]
    Connect(#[source] wayland_client::ConnectError),
    /// The compositor's socket could not be connected to.
    #[error("could not connect to {}", path.display())]
    Socket {
        /// The socket.
        path: std::path::PathBuf,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// The compositor's globals could not be listed.
    #[error("could not list the compositor's globals")]
    Globals(#[source] wayland_client::globals::GlobalError),
    /// A global the client needs could not be bound.
    #[error("could not bind {interface}")]
    Bind {
        /// The global.
        interface: &'static str,
        /// Why.
        #[source]
        source: wayland_client::globals::BindError,
    },
    /// The connection failed while waiting for the compositor.
    #[error("the connection to the compositor failed")]
    Dispatch(#[source] wayland_client::DispatchError),
    /// A request could not be sent.
    #[error("could not send to the compositor")]
    Send(#[source] wayland_client::backend::WaylandError),
    /// No output has this name.
    #[error("there is no output named {0:?}")]
    UnknownOutput(String),
    /// A window did not draw what was waited for within [`PATIENCE`].
    #[error("the window {title:?} did not draw {what} (it last drew {drawn:?})")]
    NotDrawn {
        /// The window's title.
        title: String,
        /// What was waited for.
        what: String,
        /// The size it last drew, if any.
        drawn: Option<(i32, i32)>,
    },
    /// A window's buffer could not be made.
    #[error("could not make a window's buffer: {0}")]
    Buffer(String),
    /// The compositor does not offer `zwlr_output_manager_v1` (KWin, for
    /// one; see [`configure_output`]).
    #[error("the compositor does not offer zwlr_output_manager_v1")]
    NoOutputManagement,
    /// The compositor refused an output change.
    #[error("the compositor refused to change {name}: {reason}")]
    OutputRefused {
        /// The output.
        name: String,
        /// Why, as far as it said.
        reason: String,
    },
}

/// Locks `mutex`, through a panic of another holder: the state it guards is
/// plain values, valid at every step.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

//! Throwaway Wayland clients for deskrinch's compositor tests: windows of
//! known content, and changes to the session's outputs.
//!
//! - [`TestClient`] is a connection of its own, dispatched on its own
//!   thread so that it answers pings and configures like a live
//!   application. Its [`TestWindow`]s are mapped `xdg_toplevel`s that draw
//!   a [`Paint`] (nothing, one colour, or four quarters) at whatever size
//!   the compositor asks for, with a buffer scale of 1.
//! - The keyboard: the client takes the seat's keyboard whenever the seat
//!   has one (on a headless wlroots compositor, only once
//!   `deskrinch-test-input` has made a virtual keyboard). A window notes
//!   whether it has the keyboard focus ([`TestWindow::has_keyboard`],
//!   [`TestWindow::wait_keyboard`]) and records each key pressed while it
//!   does, by evdev code, in order ([`TestWindow::keys`],
//!   [`TestWindow::wait_key`]). That is all it does with the keyboard: a
//!   release, a repeat, the keymap and the modifiers are not kept, and keys
//!   pressed while another window has the focus are not its own.
//! - The pointer: the client takes the seat's pointer whenever the seat has
//!   one. A window records what it hears of it while the pointer is over
//!   it, in its surface coordinates and in order: entering, moving, buttons
//!   pressed and released, leaving ([`PointerEvent`],
//!   [`TestWindow::pointer_events`], [`TestWindow::wait_pointer_at`]). Wheel
//!   events, frames and the cursor are not kept.
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
mod keyboard;
mod output_info;
mod outputs;
mod paint;
mod pointer;
mod seat;
mod window;

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

pub use client::TestClient;
pub use compositor::Compositor;
pub use output_info::OutputInfo;
pub use outputs::{OutputChange, OutputSettings, configure_every_output, configure_output};
pub use paint::Paint;
pub use pointer::{PointerEvent, PointerMark};
pub use window::TestWindow;

/// How long anything here waits for the compositor (a window to be drawn
/// as asked, a round trip, an output change): generous, since a loaded
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
    /// A global the client needs could not be bound.
    #[error("could not bind a global the client needs")]
    Bind(#[source] deskrinch_test_wayland::BindError),
    /// The compositor did not answer within [`PATIENCE`].
    #[error("the compositor did not answer within {patience:?} while {what}")]
    TimedOut {
        /// What the client was waiting for.
        what: &'static str,
        /// How long it waited.
        patience: Duration,
    },
    /// The connection failed while waiting for the compositor.
    #[error("the connection to the compositor failed")]
    Connection(#[source] deskrinch_test_wayland::WaitError),
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
    /// A window did not receive the input waited for within [`PATIENCE`].
    #[error("the window {title:?} did not receive {what}")]
    NotReceived {
        /// The window's title.
        title: String,
        /// What was waited for.
        what: String,
    },
    /// A window's buffer could not be made.
    #[error("could not make a window's buffer")]
    Buffer(#[source] deskrinch_test_wayland::ShmError),
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

/// The error for a wait on the compositor, while `what`, that ended in
/// `error`.
pub(crate) fn waited(error: deskrinch_test_wayland::WaitError, what: &'static str) -> Error {
    match error {
        deskrinch_test_wayland::WaitError::TimedOut(patience) => Error::TimedOut { what, patience },
        other => Error::Connection(other),
    }
}

/// Locks `mutex`, through a panic of another holder: the state it guards is
/// plain values, valid at every step.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

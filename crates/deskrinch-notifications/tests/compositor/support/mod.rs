//! What the daemon's compositor tests share, by what it holds:
//!
//! - [`daemon`]: the daemon started in the session with the run's renderer
//!   and an icon theme of the test's own, and a client sending to it on the
//!   session's bus;
//! - [`screen`]: the column read from the screen;
//! - [`pointer`]: the pointer, and windows of the test's own under the
//!   column that hear it ([`Beneath`]).
//!
//! [`Notifications`] holds the three together; each part adds its own
//! methods.

mod daemon;
mod pointer;
mod screen;

use deskrinch_test_client::Compositor;
use deskrinch_test_component::Component;
use deskrinch_test_input::Input;
use deskrinch_test_notify::Client;
use deskrinch_test_screen::Screen;
use tempfile::TempDir;

pub(crate) use daemon::{RED_ICON, RED_ICON_NAME};
pub(crate) use pointer::{Beneath, clicked_since, over_since, pressed_since};
pub(crate) use screen::{Column, close_button, find, reddish, whole};

/// The theme's spacing the column is padded with (`--rinch-spacing-sm`),
/// in logical pixels: the popups start this far from the output's edges.
pub(crate) const PADDING: u32 = 12;

/// The column's width in logical pixels, its padding included: the
/// daemon's.
pub(crate) const WIDTH: u32 = 380;

/// Panics unless the tests run in a nested session started by `cargo xtask
/// compositor-tests`: run anywhere else they would take the real session's
/// notifications and photograph its screen. Every test calls it first.
fn in_nested_session() {
    let _compositor = Compositor::current();
}

/// The daemon in a session, a client of it, and the screen and pointer.
pub(crate) struct Notifications {
    _data: TempDir,
    daemon: Component,
    pub(crate) client: Client,
    screen: Screen,
    input: Input,
    /// Every output before the daemon showed anything.
    before: screen::Pictures,
    /// The session's scale: the screen's pixels per logical pixel.
    pub(crate) scale: u32,
}

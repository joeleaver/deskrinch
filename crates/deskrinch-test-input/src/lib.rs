//! Input for deskrinch's compositor tests: keys pressed into a nested
//! session as a keyboard would press them, so the compositor hands them to
//! the window with the keyboard focus, with a serial, like real input.
//!
//! - [`Input`] is a connection of its own to the session's compositor and
//!   the device it injects through:
//!   - under sway and labwc, wlroots' `zwp_virtual_keyboard_v1` (with a US
//!     keymap). On a headless wlroots seat it is also the seat's only
//!     keyboard, so it is made before the windows that are to hear it, and
//!     kept while they should;
//!   - under KWin, `org_kde_kwin_fake_input`, a restricted interface that
//!     KWin offers only to a binary granted [`KWIN_FAKE_INPUT_INTERFACE`]
//!     (`cargo xtask compositor-tests` grants it through its suites table).
//! - [`Key`]: a key by its evdev code, and the text a US layout types
//!   without a modifier.
//!
//! Only the keyboard is here. A pointer goes through the same two
//! mechanisms (wlroots' virtual pointer, KWin's fake input), as a second
//! device beside the keyboard in [`Input`]'s backend.
//!
//! Like the other test crates it is for nested sessions only, and refuses
//! anything else ([`deskrinch_test_client::Compositor::current`]): it would
//! type into the real desktop.

mod backend;
mod key;

pub use backend::Input;
pub use key::Key;

use std::time::Duration;

/// The restricted KWin interface keys are injected through.
pub const KWIN_FAKE_INPUT_INTERFACE: &str = "org_kde_kwin_fake_input";

/// How long anything here waits for the compositor: generous, since a
/// loaded machine runs nested compositors slowly.
pub const PATIENCE: Duration = Duration::from_secs(60);

/// Why input could not be injected.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// There is no compositor to connect to.
    #[error("could not connect to the compositor")]
    Connect(#[source] wayland_client::ConnectError),
    /// The compositor offers neither mechanism: under KWin, the binary is
    /// not granted [`KWIN_FAKE_INPUT_INTERFACE`].
    #[error(
        "the compositor offers neither zwp_virtual_keyboard_manager_v1 nor \
         org_kde_kwin_fake_input (under KWin, the test binary must be granted it)"
    )]
    NoMechanism,
    /// A global could not be bound.
    #[error("could not bind a global input needs")]
    Bind(#[source] deskrinch_test_wayland::BindError),
    /// The keymap's file could not be made.
    #[error("could not make the virtual keyboard's keymap")]
    Keymap(#[source] std::io::Error),
    /// The text has a character the keys cannot type.
    #[error("{0:?} cannot be typed without a modifier on a US layout")]
    Untypable(char),
    /// The compositor did not answer, or the connection failed.
    #[error("waiting for the compositor failed")]
    Wait(#[source] deskrinch_test_wayland::WaitError),
}

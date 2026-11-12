//! Input for deskrinch's compositor tests: keys pressed and a pointer moved
//! and clicked in a nested session, as a keyboard and a mouse would, so the
//! compositor hands them to the window with the keyboard focus or under the
//! pointer, with serials, like real input.
//!
//! - [`Input`] is a connection of its own to the session's compositor and
//!   the devices it injects through:
//!   - under sway and labwc, wlroots' `zwp_virtual_keyboard_v1` (with a US
//!     keymap) and `zwlr_virtual_pointer_v1`. On a headless wlroots seat
//!     they are the seat's only keyboard and pointer: the seat says it has
//!     neither until they exist, then says so again to every client, so a
//!     window that connected earlier takes them then. The pointer reaches
//!     such a window like any other. The keyboard is made before the
//!     windows that are to hear it all the same, since a compositor gives a
//!     window the keyboard focus as it maps, and a seat without a keyboard
//!     gives none;
//!   - under KWin, `org_kde_kwin_fake_input`, a restricted interface that
//!     KWin offers only to a binary granted [`KWIN_FAKE_INPUT_INTERFACE`]
//!     (`cargo xtask compositor-tests` grants it through its suites table).
//! - [`Key`]: a key by its evdev code, and the text a US layout types
//!   without a modifier; a key pressed with modifiers held, as a chord
//!   ([`Input::press_with`]).
//! - The pointer ([`Input::move_pointer`], [`Input::press_button`],
//!   [`Input::release_button`], [`Input::click`]): moved to a point on a
//!   named output, in that output's logical pixels from its top-left
//!   corner, the names being the ones `deskrinch-test-client` and
//!   `deskrinch-test-screen` use; its [`Button`]s pressed and released
//!   where it is. A point is on one output, so every window on the screen,
//!   whichever client's, can be reached.
//!
//! # Where a point goes
//!
//! A point is named on an output in that output's logical pixels: the
//! coordinates a fullscreen window on it has, whatever the output's scale
//! and transform. Neither mechanism takes that:
//!
//! - KWin's `pointer_motion_absolute` takes the compositor's global
//!   coordinates, the logical layout the outputs share: the output's
//!   position plus the point.
//! - wlroots' `motion_absolute` takes a position against an extent on each
//!   axis, which the compositor maps onto the box around every output in
//!   the layout (the virtual pointer is mapped to no one output). The
//!   global point is given against that box, in 256ths of a logical pixel.
//!
//! Each output's position and size in the layout are what `xdg-output`
//! reports (`deskrinch-test-client`'s
//! [`OutputInfo`](deskrinch_test_client::OutputInfo)), read each time the
//! pointer moves, so a reconfigured output is placed where it now is.
//!
//! An `Input` leaves nothing behind it. A button pressed with
//! [`Input::press_button`] and not released is released when the `Input` is
//! dropped (a test that fails mid-drag included), and the keys have no way to
//! be left down, since [`Input::press`], [`Input::press_with`] and
//! [`Input::type_text`] release every key they press. Its devices are destroyed with it, and are gone
//! when the drop returns: the next `Input`'s are never made while the
//! seat is still taking these away, which would take the pointer from every
//! window for a moment.
//!
//! What the pointer cannot do: scroll (no wheel or axis events), touch, or
//! move relatively; it moves straight to the point, so nothing between the
//! last point and the next sees it pass.
//!
//! Like the other test crates it is for nested sessions only, and refuses
//! anything else ([`deskrinch_test_client::Compositor::current`]): it would
//! type into, and click on, the real desktop.

mod backend;
mod button;
mod key;
mod layout;
mod pointer;

pub use backend::Input;
pub use button::Button;
pub use key::Key;

use std::time::Duration;

/// The restricted KWin interface keys and the pointer are injected through.
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
    /// The client that learns where the outputs are could not connect.
    #[error("could not learn the outputs' layout")]
    Outputs(#[source] deskrinch_test_client::Error),
    /// A wlroots compositor without `zwlr_virtual_pointer_manager_v1`.
    #[error("the compositor offers no zwlr_virtual_pointer_manager_v1: there is no pointer")]
    NoPointer,
    /// There is no output so named.
    #[error("there is no output named {0:?}")]
    NoOutput(String),
    /// The point is not on the output.
    #[error("({x}, {y}) is not on {output:?}, which is {size:?} logical pixels")]
    OffOutput {
        /// The output named.
        output: String,
        /// The point's x, in the output's logical pixels.
        x: f64,
        /// The point's y.
        y: f64,
        /// The output's logical size.
        size: (i32, i32),
    },
    /// An output whose place in the layout has not been reported.
    #[error("the compositor has not said where {0:?} is in the layout")]
    Unplaced(String),
}

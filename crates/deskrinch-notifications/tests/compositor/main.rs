//! The `deskrinch-notifications` binary against real compositors: nested
//! KWin, sway or labwc, started by `cargo xtask compositor-tests`, which
//! runs these one at a time in each of its passes and with each renderer
//! the run tests, and grants this binary KWin's screenshot interface and
//! fake input.
//!
//! A test client sends notifications on the session's bus
//! (`deskrinch-test-notify`) and reads the signals they get back; the
//! popups are checked by the screen's pixels, and clicked through
//! `deskrinch-test-input`'s pointer.
//!
//! - [`shown`]: no window until a notification; a popup at the top right;
//!   popups stacked, the newest on top; a burst all shown.
//! - [`lifecycle`]: a popup expires, is closed by its sender, is replaced
//!   in place; the window goes with the last.
//! - [`clicks`]: the close button dismisses; an action's button and the
//!   default action are invoked and signalled; the pointer over a popup
//!   holds its expiry, and so does one that moves under a resting pointer.
//! - [`frames`]: one frame for a change, none while nothing changes.
//! - [`burst`]: changes in quick succession, every shown popup drawn whole
//!   at the end.
//! - [`room`]: more popups than fit (the shown whole, the rest counted and
//!   held), a critical one into a full column, the room shrinking under a
//!   panel and coming back, a first popup taller than the room.
//! - [`through`]: the pointer pressed on the gaps, the padding, the count
//!   line, the strip below the column and a hidden popup reaches a window
//!   under the column; pressed on a shown popup, it does not.
//!
//! The panel the room and pass-through tests start ([`panel`]) is the
//! shell's own, declared as one of this suite's programs.

// `cfg(test)` marks them as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod burst;
#[cfg(test)]
mod clicks;
#[cfg(test)]
mod frames;
#[cfg(test)]
mod lifecycle;
#[cfg(test)]
mod panel;
#[cfg(test)]
mod room;
#[cfg(test)]
mod shown;
#[cfg(test)]
mod support;
#[cfg(test)]
mod through;

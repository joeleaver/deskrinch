//! `deskrinch-workspaces` against a real compositor: nested KWin, sway or
//! labwc, started by `cargo xtask compositor-tests`, which runs the tests
//! one at a time inside the session (its own display and bus).
//!
//! - [`listing`]: the workspaces listed, in order, with the active one.
//! - [`switching`]: a switch made through the crate seen by the compositor,
//!   and one made by the compositor's own means seen through the crate.
//! - [`desktops`]: KWin's desktops created, removed and laid out in rows by
//!   KWin's own interface.
//! - [`shared`]: workspaces and window management on one connection.
//! - [`unsupported`]: sway, which offers no workspace protocol.
//!
//! KWin's side is its D-Bus interface (`org.kde.KWin.VirtualDesktopManager`
//! on the session's bus); labwc's is its F12 binding (the sessions' labwc
//! configuration), pressed on `deskrinch-test-input`'s virtual keyboard, and
//! what the screen shows.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod desktops;
#[cfg(test)]
mod listing;
#[cfg(test)]
mod shared;
#[cfg(test)]
mod support;
#[cfg(test)]
mod switching;
#[cfg(test)]
mod unsupported;

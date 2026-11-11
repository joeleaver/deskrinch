//! The `deskrinch-launcher` binary against a real compositor: nested KWin,
//! sway or labwc, started by `cargo xtask compositor-tests`, which grants
//! this binary KWin's screenshot interface and fake input, and runs the
//! tests one at a time inside the session (its own display and bus).
//!
//! - [`process`]: the process, through the bus and its frames, without
//!   looking at the screen.
//! - [`screen`]: keys typed into it, what it draws, and what it starts.
//!
//! Every launcher a test starts is ended before the test returns, the one a
//! client starts (in a process group of its own) included.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod process;
#[cfg(test)]
mod screen;
#[cfg(test)]
mod support;

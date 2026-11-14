//! `deskrinch-wayland-client` against a real compositor: nested KWin, sway
//! or labwc, started by `cargo xtask compositor-tests`.
//!
//! - [`stress`]: states attached and detached on one client from several
//!   threads at once, many times, every wait bounded so that a hang fails.
//! - [`lifecycle`]: the thread ends with the last handle; a panicking
//!   handler ends every state; a handle dropped from inside its own handler
//!   does not deadlock.
//! - [`release`]: a detached state released at once, or kept releasing
//!   while something can reach it; the connection closing with its last
//!   handle regardless; a releasing state's panic ending the others.
//! - [`registry`]: a global that goes and comes back after attaching
//!   reaches every state, through the client's one registry.
//!
//! The states here are the tests' own ([`support`]): each binds the outputs
//! and records how it ended.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod lifecycle;
#[cfg(test)]
mod registry;
#[cfg(test)]
mod release;
#[cfg(test)]
mod stress;
#[cfg(test)]
mod support;

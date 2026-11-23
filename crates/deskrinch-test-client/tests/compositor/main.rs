//! The test client's output changes against real compositors: nested KWin,
//! sway or labwc, with two outputs, started by `cargo xtask
//! compositor-tests`.
//!
//! - [`close_then_open`]: a window closed and the next opened at once,
//!   four thousand times: the client keeps to the protocol (run under
//!   labwc alone, by its own row of the suites table).
//! - [`layer`]: layer surfaces with a chosen namespace, layer and size.
//! - [`layout`]: every change is put back exactly, the whole layout: each
//!   output turned off and put back, turned and put back, scaled and put
//!   back, and a guard
//!   dropped (by the test, or by its panic) instead of restored.
//! - [`entered`]: a window says which output it is on.
//! - [`uhd_beside_full_hd`]: the outputs of the pass that has its own,
//!   which its own row alone runs.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod close_then_open;
#[cfg(test)]
mod entered;
#[cfg(test)]
mod layer;
#[cfg(test)]
mod layout;
#[cfg(test)]
mod uhd_beside_full_hd;

//! The test client's output changes against real compositors: nested KWin,
//! sway or labwc, with two outputs, started by `cargo xtask
//! compositor-tests`.
//!
//! - [`layer`]: layer surfaces with a chosen namespace, layer and size.
//! - [`layout`]: every change is put back exactly, the whole layout: each
//!   output turned off and put back, turned and put back, scaled and put
//!   back, and a guard
//!   dropped (by the test, or by its panic) instead of restored.
//! - [`uhd_beside_full_hd`]: the outputs of the pass that has its own,
//!   which its own row alone runs.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod layer;
#[cfg(test)]
mod layout;
#[cfg(test)]
mod uhd_beside_full_hd;

//! The test client's output changes against real compositors: nested KWin,
//! sway or labwc, with two outputs, started by `cargo xtask
//! compositor-tests`.
//!
//! - [`layout`]: every change is put back exactly, the whole layout: each
//!   output turned off and put back, turned and put back, and a guard
//!   dropped (by the test, or by its panic) instead of restored.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod layout;

//! The crate against a real compositor: nested KWin, sway or labwc, started
//! by `cargo xtask compositor-tests`, which grants this binary KWin's fake
//! input and runs the tests one at a time, in every pass (scale 1 and 2,
//! upright, turned and flipped). Each test opens windows of its own
//! (`deskrinch-test-client`) and types into them, or points at them.
//!
//! - [`typing`]: keys reach the window with the keyboard focus.
//! - [`pointer`]: the pointer reaches the window under a point on a named
//!   output, at that point, with its buttons.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod pointer;
#[cfg(test)]
mod typing;

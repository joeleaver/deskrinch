//! The crate against real compositors: a window of known colours and size
//! is put on an output, captured, and compared pixel for pixel.
//!
//! Ignored by `cargo test`; `cargo xtask compositor-tests` runs them inside
//! nested, headless KWin, sway and labwc sessions with two outputs, at scale
//! 1, at scale 2, and at scale 1 turned a quarter, granting this binary
//! KWin's screenshot interface the way an installed deskrinch is granted it.
//! Each test reads the scale and the transform from the output it captures. What only the wlroots compositors
//! offer is tested by the `wlroots` binary.

// `cfg(test)` marks them as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod capture;
#[cfg(test)]
#[path = "../support/mod.rs"]
mod support;

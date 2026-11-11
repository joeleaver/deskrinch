//! The crate against real compositors: a window of known colours and size
//! is put on an output, captured, and compared pixel for pixel.
//!
//! Ignored by `cargo test`; `cargo xtask compositor-tests` runs them inside
//! nested, headless KWin, sway and labwc sessions with two outputs, once at
//! scale 1 and once at scale 2, granting this binary KWin's screenshot
//! interface the way an installed deskrinch is granted it. Each test reads
//! the scale from the output it captures. What only the wlroots compositors
//! offer is tested by the `wlroots` binary.

// `cfg(test)` marks them as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod capture;
#[cfg(test)]
#[path = "../support/mod.rs"]
mod support;

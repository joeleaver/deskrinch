//! KWin's own view of the windows the test client makes, in a nested KWin
//! that `cargo xtask compositor-tests kwin` starts. Ignored by a plain
//! `cargo test`; KWin only.
//!
//! - [`types`]: the window type KWin gives a toplevel, and a layer surface
//!   by its namespace, with the layer and what leaves it out.
//!
//! Every test makes two windows or more and reads the one under test among
//! them, not first; a failure names KWin's version, as the table each
//! asserts was read from one (6.6.7), and another version may differ.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod types;

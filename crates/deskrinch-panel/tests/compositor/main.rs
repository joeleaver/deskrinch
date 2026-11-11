//! The panel binary against real compositors: nested KWin, sway or labwc,
//! started by `cargo xtask compositor-tests`, which runs these one at a time
//! in each of its passes (outputs at scale 1, at scale 2, turned) and with
//! each renderer the run tests (`DESKRINCH_TEST_RENDERER`).
//!
//! The panel is checked by the screen's pixels (`deskrinch-test-screen`):
//! every output is captured before it starts, and afterwards exactly one
//! output differs, there in a band of the configured height along the
//! configured edge and nowhere else; a maximised test window stops at that
//! band; and an edit of the settings file moves and resizes the running
//! panel, a rejected one leaving it where it was. Each test starts its own
//! panel, checks that the run's renderer is what presented it, and ends it.

// `cfg(test)` marks them as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod live;
#[cfg(test)]
mod placement;
#[cfg(test)]
mod support;
#[cfg(test)]
mod zone;

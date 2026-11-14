//! The wallpaper binary against real compositors: nested KWin, sway or
//! labwc, started by `cargo xtask compositor-tests` with two outputs, which
//! runs these one at a time in each of its passes (outputs at scale 1, at
//! scale 2, turned) and with each renderer the run tests
//! (`DESKRINCH_TEST_RENDERER`).
//!
//! What is checked, by the screen's pixels (`deskrinch-test-screen`):
//!
//! - [`cover`]: every output is the configured colour edge to edge; an edit
//!   of the settings repaints every output; a picture is fitted as `fit`
//!   says; a picture that cannot be shown leaves the colour and is
//!   reported with its path.
//! - [`layers`]: a window is above the wallpaper, and so is the panel.
//! - [`outputs`]: an output turned while the wallpaper runs is covered at
//!   its new shape; one turned off loses its wallpaper and, turned on
//!   again, gets one anew, the others staying covered.
//!
//! Not covered here: a new output, never seen before, plugged in (the
//! sessions' outputs are fixed at their start; one turned off and on
//! comes back as a new `wl_output` global, which is what the wallpaper
//! follows); that the wallpaper wakes up for nothing when idle, which
//! `cargo xtask measure`'s idle window shows; and that it covers a strip a
//! panel reserves, which the panel above it hides from the screen.

// `cfg(test)` marks them as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod cover;
#[cfg(test)]
mod layers;
#[cfg(test)]
mod outputs;
#[cfg(test)]
mod support;

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
//! - [`exact`]: a picture larger than its output, covered or contained, is
//!   drawn one device pixel per pixel, as `deskrinch-picture` fitted it at
//!   the output's device size, in every pass.
//! - [`file`]: the picture's file replaced whole or rewritten in place is
//!   shown; replaced by one that cannot be shown, the last good picture
//!   stays and is said; taken away, the colour; over the wallpaper's
//!   limits (a side, the decoded size, the file's size), the colour and
//!   which limit; a picture stored turned is shown upright, and one with a
//!   colour profile converted.
//! - [`first_frame`]: each window's first frame is drawn at its output's
//!   scale.
//! - [`layers`]: a window is above the wallpaper, and so is the panel.
//! - [`outputs`]: an output turned while the wallpaper runs is covered at
//!   its new shape; one turned off (unplugged, to the wallpaper) loses its
//!   window and, turned on again (plugged in), gets exactly one new one,
//!   the other output keeping its window.
//! - [`reconfigured`]: an output moved, rescaled or turned keeps its
//!   window, and is covered at its new shape.
//! - [`together`]: with the two outputs at one position, where sway shows
//!   the wallpaper of only one, the wallpaper follows four settings edits
//!   in turn on the output shown, and both outputs show the last once
//!   apart (rinch issue 2669).
//! - [`uhd_beside_full_hd`], run only on the pass with outputs of its own
//!   (a 3840x2160 output beside a 1920x1080 one): a picture larger than
//!   both is drawn one pixel per pixel on each at its own size; one the
//!   size of the larger is the picture itself on both, the smaller scaled
//!   at paint.
//!
//! An output unplugged and one plugged in are tested as an output turned
//! off and on. To a client both are the same: a `wl_output` global removed,
//! and a new one added with a new id; the wallpaper keys and places its
//! windows by that id, never by name, so it takes the same path for both.
//! A compositor-side unplug is left out, for what each compositor can do in
//! a test session (found 2026-11-16):
//!
//! - KWin 6.6 (`--virtual`): its virtual outputs are fixed at its start
//!   (`--output-count`); nothing on its bus adds or removes one, and
//!   `kscreen-doctor` only enables, disables and configures. Off and on
//!   only.
//! - labwc 0.9: its headless outputs are fixed at its start
//!   (`WLR_HEADLESS_OUTPUTS`); it has no command that makes or destroys
//!   one, and `wlr-output-management` only enables and disables. Off and on
//!   only.
//! - sway 1.11: `swaymsg output <name> unplug` and `swaymsg create_output`
//!   work, but a test cannot use them here. The output it makes has a new
//!   name (`HEADLESS-3`, never `HEADLESS-2` again), so the session's layout
//!   cannot be put back as the test client's guard and the runner's check
//!   expect. And an unplug after any output's mode was committed in the
//!   session (which every output change of the test client's does, the
//!   ones this suite makes among them) is a use-after-free in sway and
//!   wlroots 0.19.2 (rinch issue 2707, rinch PR 2711) that may end a
//!   client's connection, by luck of what the freed memory holds.
//!
//! Not covered here: that the wallpaper wakes up for nothing when idle,
//! which `cargo xtask measure`'s idle window shows; and that it covers a
//! strip a panel reserves, which the panel above it hides from the screen.

// `cfg(test)` marks them as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod cover;
#[cfg(test)]
mod exact;
#[cfg(test)]
mod file;
#[cfg(test)]
mod first_frame;
#[cfg(test)]
mod layers;
#[cfg(test)]
mod outputs;
#[cfg(test)]
mod reconfigured;
#[cfg(test)]
mod support;
#[cfg(test)]
mod together;
#[cfg(test)]
mod uhd_beside_full_hd;

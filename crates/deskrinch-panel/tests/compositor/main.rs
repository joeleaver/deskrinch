//! The panel binary against real compositors: nested KWin, sway or labwc,
//! started by `cargo xtask compositor-tests`, which runs these one at a time
//! in each of its passes (outputs at scale 1, at scale 2, turned) and with
//! each renderer the run tests (`DESKRINCH_TEST_RENDERER`).
//!
//! The panel is checked by the screen's pixels (`deskrinch-test-screen`):
//! every output is captured before it starts, and afterwards every output
//! differs, each in a band of the configured height along the configured
//! edge and nowhere else (a bar per output); a maximised test window stops at that
//! band; the taskbar lists the windows, its buttons' clicks act on them
//! (`taskbar_clicks`), and the button under the pointer is marked until the
//! pointer leaves (`taskbar_hover`); an edit of the settings file moves and resizes
//! the running panel, a rejected one leaving it where it was; and under a
//! fullscreen window the panel follows four edits in turn, each said by its
//! main thread, and is drawn as they left it once uncovered (rinch issue
//! 2669). Each test starts its own panel, checks that the run's renderer is
//! what presented it, and ends it.

// `cfg(test)` marks them as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod covered;
#[cfg(test)]
mod first_frame;
#[cfg(test)]
mod frames;
#[cfg(test)]
mod live;
#[cfg(test)]
mod outputs;
#[cfg(test)]
mod placement;
#[cfg(test)]
mod support;
#[cfg(test)]
mod switcher;
#[cfg(test)]
mod taskbar;
#[cfg(test)]
mod taskbar_clicks;
#[cfg(test)]
mod taskbar_hover;
#[cfg(test)]
mod zone;

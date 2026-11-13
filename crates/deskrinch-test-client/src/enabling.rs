//! Changing a nested session's output from a test under any of the three
//! compositors: through `wlr-output-management` under sway and labwc, and
//! through `kscreen-doctor` under KWin.

use crate::{Compositor, Error, OutputChange, kscreen, outputs};

/// Turns the output named `name` of the nested compositor on or off, and
/// returns once the compositor has done it. Clients learn of it a moment
/// later, through the registry: the output's `wl_output` global goes, or
/// comes back.
///
/// # Errors
///
/// As [`configure_output`](crate::configure_output) under sway and labwc;
/// under KWin, when `kscreen-doctor` cannot be run or fails.
///
/// # Panics
///
/// Outside a nested session, as [`Compositor::current`].
pub fn set_output_enabled(name: &str, enabled: bool) -> Result<(), Error> {
    match Compositor::current() {
        Compositor::Sway | Compositor::Labwc => outputs::set_enabled(name, enabled),
        Compositor::Kwin => kscreen::apply(name, &[kscreen::enabled_setting(name, enabled)]),
    }
}

/// Changes the scale or transform of the output named `name` of the nested
/// compositor, and returns once the compositor has applied it. Clients
/// learn of it a moment later, through their own `wl_output` events.
/// Unlike [`configure_output`](crate::configure_output), it does not say
/// what the output was before: a test that puts it back knows.
///
/// # Errors
///
/// As [`set_output_enabled`].
///
/// # Panics
///
/// Outside a nested session, as [`Compositor::current`].
pub fn change_output(name: &str, change: OutputChange) -> Result<(), Error> {
    match Compositor::current() {
        Compositor::Sway | Compositor::Labwc => outputs::configure_output(name, change).map(drop),
        Compositor::Kwin => kscreen::apply(name, &kscreen::kscreen_settings(name, change)),
    }
}

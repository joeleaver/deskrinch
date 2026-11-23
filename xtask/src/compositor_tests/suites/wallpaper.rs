//! The wallpaper's row.

use super::{ALL, Grant, Pass, Suite};
use crate::compositor_tests::build::{Backend, Target};

const PACKAGE: &str = "deskrinch-wallpaper";

/// The wallpaper's real binary, checked by the screen on every output; the
/// panel, declared as its program, built with it and handed to its tests,
/// is started over it. Not in the flipped pass, for the panel's reason: a
/// full output of one colour is the same mirrored.
pub(super) const WALLPAPER: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: Some(Grant {
        id: "org.deskrinch.WallpaperTests",
        name: "deskrinch wallpaper tests",
        wayland: &[],
        dbus: &[deskrinch_test_screen::KWIN_SCREENSHOT_INTERFACE],
    }),
    compositors: ALL,
    passes: &[Pass::Plain, Pass::Scaled, Pass::Rotated],
    renders: true,
    // The wallpaper the tests start, and the panel started over it.
    programs: &[&deskrinch_shell::WALLPAPER, &deskrinch_shell::PANEL],
    filter: &[],
};

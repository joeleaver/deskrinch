//! The wallpaper's rows: its tests on the ordinary passes, and its
//! pictures on a 3840x2160 output beside a 1920x1080 one.

use super::{ALL, Grant, Output, Pass, Suite};
use crate::compositor_tests::build::{Backend, Target};

const PACKAGE: &str = "deskrinch-wallpaper";
/// The wallpaper's tests of [`Pass::UhdBesideFullHd`]'s outputs: run by
/// that pass's row alone.
const WALLPAPER_UHD_BESIDE_FULL_HD_TESTS: &str = "uhd_beside_full_hd::";

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
    filter: &["--skip", WALLPAPER_UHD_BESIDE_FULL_HD_TESTS],
    output: Output::Captured,
};

/// The same binary's tests of a 3840x2160 output beside a 1920x1080 one:
/// a picture made smaller for each output, and one shown as itself on one
/// output and scaled on the other. Granted by [`WALLPAPER`]'s grant.
pub(super) const WALLPAPER_UHD_BESIDE_FULL_HD: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: ALL,
    passes: &[Pass::UhdBesideFullHd],
    renders: true,
    programs: &[&deskrinch_shell::WALLPAPER, &deskrinch_shell::PANEL],
    filter: &[WALLPAPER_UHD_BESIDE_FULL_HD_TESTS],
    output: Output::Captured,
};

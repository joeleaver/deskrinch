//! The panel's rows: its tests on the ordinary passes, its last output
//! going and coming back, the windows sway never places, and a full
//! taskbar on a wide output.

use super::{ALL, Grant, Output, PLAIN, Pass, Suite, WLROOTS};
use crate::compositor_tests::build::{Backend, Target};
use crate::session::Compositor;

const PACKAGE: &str = "deskrinch-panel";
/// The panel's test of its last output going and coming back: run by its
/// own row, under the compositors that allow it.
const PANEL_LAST_OUTPUT_TESTS: &str = "last_output::";
/// The panel's tests of the windows sway never places: run by their own
/// row, under sway.
const PANEL_SWAY_UNPLACED_TESTS: &str = "sway_unplaced::";
/// The panel's tests of a full taskbar on [`Pass::UhdBesideFullHd`]'s wide
/// output: run by that pass's row alone.
const PANEL_WIDE_TESTS: &str = "uhd_beside_full_hd::";

/// The panel's real binary, checked by the screen: it captures under
/// KWin. Not in the flipped pass: a full-width band along an edge is the
/// same mirrored, so that pass adds nothing a turned output does not.
pub(super) const PANEL: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: Some(Grant {
        id: "org.deskrinch.PanelTests",
        name: "deskrinch panel tests",
        // The taskbar's tests click its buttons, and see what the clicks
        // did to the windows.
        wayland: &[
            deskrinch_test_input::KWIN_FAKE_INPUT_INTERFACE,
            deskrinch_window_management::KWIN_INTERFACE,
        ],
        dbus: &[deskrinch_test_screen::KWIN_SCREENSHOT_INTERFACE],
    }),
    compositors: ALL,
    passes: &[Pass::Plain, Pass::Scaled, Pass::Rotated],
    renders: true,
    // The panel itself, declared so that it is granted what the shell
    // grants it: its taskbar follows the window list.
    programs: &[&deskrinch_shell::PANEL],
    filter: &[
        "--skip",
        PANEL_WIDE_TESTS,
        "--skip",
        PANEL_LAST_OUTPUT_TESTS,
        "--skip",
        PANEL_SWAY_UNPLACED_TESTS,
    ],
    output: Output::Captured,
};

/// The same binary's test of the last output going and coming back, under
/// the compositors that let a nested session turn it off.
pub(super) const PANEL_LAST_OUTPUT: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: WLROOTS,
    passes: PLAIN,
    renders: true,
    programs: &[&deskrinch_shell::PANEL],
    filter: &[PANEL_LAST_OUTPUT_TESTS, "--skip", PANEL_WIDE_TESTS],
    output: Output::Captured,
};

/// The same binary's tests of the windows sway never places (a window
/// opened on a workspace not shown, or fullscreen), under sway.
pub(super) const PANEL_SWAY_UNPLACED: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: &[Compositor::Sway],
    passes: PLAIN,
    renders: true,
    programs: &[&deskrinch_shell::PANEL],
    filter: &[PANEL_SWAY_UNPLACED_TESTS, "--skip", PANEL_WIDE_TESTS],
    output: Output::Captured,
};

/// The same binary's full taskbar on a wide output, on those outputs
/// alone.
pub(super) const PANEL_UHD_BESIDE_FULL_HD: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: ALL,
    passes: &[Pass::UhdBesideFullHd],
    renders: true,
    programs: &[&deskrinch_shell::PANEL],
    filter: &[PANEL_WIDE_TESTS],
    output: Output::Captured,
};

//! The rows of the test-support crates: the test client's output
//! changes, the screen's captures and the input devices.

use super::{ALL, Grant, KWIN, PLAIN, Pass, Suite, WLROOTS};
use crate::compositor_tests::build::{Backend, Target};

const TEST_CLIENT_PACKAGE: &str = "deskrinch-test-client";
const SCREEN_PACKAGE: &str = "deskrinch-test-screen";
const INPUT_PACKAGE: &str = "deskrinch-test-input";
/// The test client's tests of [`Pass::UhdBesideFullHd`]'s outputs: run by
/// that pass's row alone.
const UHD_BESIDE_FULL_HD_TESTS: &str = "uhd_beside_full_hd::";

/// The test client's output changes, first: that every change to the
/// session's outputs is put back exactly, which the suites after it
/// rely on.
pub(super) const TEST_CLIENT: Suite = Suite {
    target: Target {
        package: TEST_CLIENT_PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: ALL,
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &["--skip", UHD_BESIDE_FULL_HD_TESTS],
};

/// The same binary's tests of a 3840x2160 output beside a 1920x1080
/// one, on those outputs alone.
pub(super) const TEST_CLIENT_UHD_BESIDE_FULL_HD: Suite = Suite {
    target: Target {
        package: TEST_CLIENT_PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: ALL,
    passes: &[Pass::UhdBesideFullHd],
    renders: false,
    programs: &[],
    filter: &[UHD_BESIDE_FULL_HD_TESTS],
};

pub(super) const SCREEN: Suite = Suite {
    target: Target {
        package: SCREEN_PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: Some(Grant {
        id: "org.deskrinch.TestScreenTests",
        name: "deskrinch screen-capture tests",
        wayland: &[],
        dbus: &[deskrinch_test_screen::KWIN_SCREENSHOT_INTERFACE],
    }),
    compositors: ALL,
    passes: &Pass::ALL,
    renders: false,
    programs: &[],
    filter: &[],
};

pub(super) const SCREEN_WLROOTS: Suite = Suite {
    target: Target {
        package: SCREEN_PACKAGE,
        test: "wlroots",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: WLROOTS,
    // It rotates an output itself.
    passes: &[Pass::Plain, Pass::Scaled],
    renders: false,
    programs: &[],
    filter: &[],
};

pub(super) const SCREEN_NOT_GRANTED: Suite = Suite {
    target: Target {
        package: SCREEN_PACKAGE,
        test: "kwin_not_granted",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: KWIN,
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &[],
};

/// The input devices, typing into test windows and pointing at them:
/// under KWin through its restricted fake input. Every pass, since where
/// the pointer lands depends on the outputs' scale and transform.
pub(super) const INPUT: Suite = Suite {
    target: Target {
        package: INPUT_PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: Some(Grant {
        id: "org.deskrinch.TestInputTests",
        name: "deskrinch input tests",
        wayland: &[deskrinch_test_input::KWIN_FAKE_INPUT_INTERFACE],
        dbus: &[],
    }),
    compositors: ALL,
    passes: &Pass::ALL,
    renders: false,
    programs: &[],
    filter: &[],
};

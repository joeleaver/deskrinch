//! The launcher's row.

use super::{ALL, Grant, PLAIN, Suite};
use crate::compositor_tests::build::{Backend, Target};

const PACKAGE: &str = "deskrinch-launcher";

/// The launcher, its real binary: its process, and keys typed into it
/// with what it draws; once per renderer of the run.
pub(super) const LAUNCHER: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: Some(Grant {
        id: "org.deskrinch.LauncherTests",
        name: "deskrinch launcher tests",
        wayland: &[deskrinch_test_input::KWIN_FAKE_INPUT_INTERFACE],
        dbus: &[deskrinch_test_screen::KWIN_SCREENSHOT_INTERFACE],
    }),
    compositors: ALL,
    passes: PLAIN,
    renders: true,
    // The launcher the tests start, and run again as a client.
    programs: &[&deskrinch_shell::LAUNCHER],
    filter: &[],
};

//! The window list's rows: on the Rust backend, refused where KWin
//! grants nothing, on libwayland, and the session's end.

use super::{ALL, Grant, KWIN, Output, PLAIN, SESSION_END, Suite};
use crate::compositor_tests::build::{Backend, Target};

const PACKAGE: &str = "deskrinch-window-management";

pub(super) const WINDOW_MANAGEMENT: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: Some(Grant {
        id: "org.deskrinch.WindowManagementTests",
        name: "deskrinch window-management tests",
        wayland: &[deskrinch_window_management::KWIN_INTERFACE],
        dbus: &[],
    }),
    compositors: ALL,
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &["--skip", SESSION_END],
    output: Output::Captured,
};

pub(super) const WINDOW_MANAGEMENT_NOT_GRANTED: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "kwin_not_granted",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: KWIN,
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &[],
    output: Output::Captured,
};

pub(super) const WINDOW_MANAGEMENT_ON_LIBWAYLAND: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::System,
    },
    grant: Some(Grant {
        id: "org.deskrinch.WindowManagementTestsLibwayland",
        name: "deskrinch window-management tests on libwayland",
        wayland: &[deskrinch_window_management::KWIN_INTERFACE],
        dbus: &[],
    }),
    compositors: ALL,
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &["--skip", SESSION_END],
    output: Output::Captured,
};

/// Last: it ends the session.
pub(super) const WINDOW_MANAGEMENT_SESSION_END: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: ALL,
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &[SESSION_END],
    output: Output::Captured,
};

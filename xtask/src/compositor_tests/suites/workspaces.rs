//! The workspaces' rows: by compositor, as each offers them, and their
//! shared-connection tests on libwayland.

use super::{Grant, KWIN, PLAIN, Suite};
use crate::compositor_tests::build::{Backend, Target};
use crate::session::Compositor;

const PACKAGE: &str = "deskrinch-workspaces";
/// The workspace tests that need a compositor with no workspace protocol
/// (sway), and those that change KWin's desktops by KWin's own interface.
const UNSUPPORTED: &str = "unsupported::";
const KWIN_DESKTOPS: &str = "desktops::";
/// The workspace tests that share a connection with window management.
const SHARED: &str = "shared::";

/// The workspaces. KWin offers its virtual-desktop protocol ungranted, so
/// the grant lists only what the tests also use: window management on a
/// shared connection, and screenshots. labwc's workspaces are fixed by
/// its configuration (four), so KWin's `desktops::` tests do not run
/// there; sway offers no workspace protocol, which `unsupported::`
/// checks, alone.
pub(super) const WORKSPACES_KWIN: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: Some(Grant {
        id: "org.deskrinch.WorkspacesTests",
        name: "deskrinch workspaces tests",
        wayland: &[deskrinch_window_management::KWIN_INTERFACE],
        dbus: &[deskrinch_test_screen::KWIN_SCREENSHOT_INTERFACE],
    }),
    compositors: KWIN,
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &["--skip", UNSUPPORTED],
};

pub(super) const WORKSPACES_LABWC: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: &[Compositor::Labwc],
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &["--skip", UNSUPPORTED, "--skip", KWIN_DESKTOPS],
};

pub(super) const WORKSPACES_UNSUPPORTED: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: &[Compositor::Sway],
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &[UNSUPPORTED],
};

/// The workspaces' shared-connection tests on libwayland too: they share
/// one connection with window management.
pub(super) const WORKSPACES_SHARED_ON_LIBWAYLAND: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::System,
    },
    grant: Some(Grant {
        id: "org.deskrinch.WorkspacesTestsLibwayland",
        name: "deskrinch workspaces tests on libwayland",
        wayland: &[deskrinch_window_management::KWIN_INTERFACE],
        dbus: &[],
    }),
    compositors: &[Compositor::Kwin, Compositor::Labwc],
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &[SHARED],
};

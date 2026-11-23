//! xtask's own rows: `cargo xtask desktop` nested, measure's input
//! client, and each shell component's end.

use super::{ALL, Grant, Output, PLAIN, Suite};
use crate::compositor_tests::build::{Backend, Target};

const PACKAGE: &str = "xtask";

/// xtask's input client test, whose suite grants xtask's own binary too.
pub(in super::super) const INPUT_CLIENT: Target = Target {
    package: PACKAGE,
    test: "input_client",
    backend: Backend::Rust,
};

/// `cargo xtask desktop`, nested: its window opens in this session, which
/// the binary closes through window management and types the launcher's
/// chord into. Its output is shown, for what the edge test found inside
/// the desktop (the desktop's own KWin's compositing type among it).
pub(super) const DESKTOP: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "desktop",
        backend: Backend::Rust,
    },
    grant: Some(Grant {
        id: "org.deskrinch.DesktopTests",
        name: "deskrinch desktop tests",
        wayland: &[
            deskrinch_window_management::KWIN_INTERFACE,
            deskrinch_test_input::KWIN_FAKE_INPUT_INTERFACE,
        ],
        dbus: &[],
    }),
    compositors: ALL,
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &[],
    output: Output::Shown,
};

/// `cargo xtask input`, measure's input client: its lines reach a window
/// of the test's own. Under KWin the client, xtask's own binary, is
/// granted fake input with the suite (`granted.rs`).
pub(super) const XTASK_INPUT: Suite = Suite {
    target: INPUT_CLIENT,
    grant: Some(Grant {
        id: "org.deskrinch.InputClientTests",
        name: "deskrinch input client tests",
        wayland: &[deskrinch_test_input::KWIN_FAKE_INPUT_INTERFACE],
        dbus: &[],
    }),
    compositors: ALL,
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &[],
    output: Output::Captured,
};

/// Each shell component's end when its display goes away, in a session
/// of its own nested in this one, whose compositor the test kills.
pub(super) const DISPLAY_LOST: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "display_lost",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: ALL,
    passes: PLAIN,
    renders: false,
    programs: &[
        &deskrinch_shell::WALLPAPER,
        &deskrinch_shell::NOTIFICATIONS,
        &deskrinch_shell::LAUNCHER,
        &deskrinch_shell::PANEL,
    ],
    filter: &[],
    output: Output::Captured,
};

/// Each shell component's end when it is asked to stop (`SIGTERM`,
/// `SIGINT`), in a session of its own nested in this one.
pub(super) const ASKED_TO_STOP: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "asked_to_stop",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: ALL,
    passes: PLAIN,
    renders: false,
    programs: &[
        &deskrinch_shell::WALLPAPER,
        &deskrinch_shell::NOTIFICATIONS,
        &deskrinch_shell::LAUNCHER,
        &deskrinch_shell::PANEL,
    ],
    filter: &[],
    output: Output::Captured,
};

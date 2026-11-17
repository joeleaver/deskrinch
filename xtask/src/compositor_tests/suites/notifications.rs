//! The notification daemon's row.

use super::{ALL, Grant, Pass, Suite};
use crate::compositor_tests::build::{Backend, Target};

const PACKAGE: &str = "deskrinch-notifications";

/// The notification daemon's real binary, sent notifications on the
/// session's bus, its popups checked by the screen and clicked with the
/// pointer. Not in the flipped pass, for the panel's reason: a turned
/// output already moves the corner the column is anchored to.
pub(super) const NOTIFICATIONS: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: Some(Grant {
        id: "org.deskrinch.NotificationsTests",
        name: "deskrinch notifications tests",
        wayland: &[deskrinch_test_input::KWIN_FAKE_INPUT_INTERFACE],
        dbus: &[deskrinch_test_screen::KWIN_SCREENSHOT_INTERFACE],
    }),
    compositors: ALL,
    passes: &[Pass::Plain, Pass::Scaled, Pass::Rotated],
    renders: true,
    // The daemon, and the panel the room tests start along the top.
    programs: &[&deskrinch_shell::NOTIFICATIONS, &deskrinch_shell::PANEL],
    filter: &[],
};

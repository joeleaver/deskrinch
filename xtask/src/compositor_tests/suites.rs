//! The test binaries `cargo xtask compositor-tests` runs, and where.

use super::build::Target;
use crate::session::Compositor;

const WINDOW_MANAGEMENT: &str = "deskrinch-window-management";
const SCREEN: &str = "deskrinch-test-screen";
const INPUT: &str = "deskrinch-test-input";
const LAUNCHER: &str = "deskrinch-launcher";

const ALL: &[Compositor] = &[Compositor::Kwin, Compositor::Sway, Compositor::Labwc];
const KWIN: &[Compositor] = &[Compositor::Kwin];
const WLROOTS: &[Compositor] = &[Compositor::Sway, Compositor::Labwc];

/// What KWin grants a test binary.
#[derive(Debug)]
pub(super) struct Grant {
    /// The desktop file id.
    pub(super) id: &'static str,
    pub(super) name: &'static str,
    pub(super) wayland: &'static [&'static str],
    pub(super) dbus: &'static [&'static str],
}

/// A test binary and how it runs.
#[derive(Debug)]
pub(super) struct Suite {
    pub(super) target: Target,
    /// What KWin grants it; nothing for a binary that checks a refusal.
    pub(super) grant: Option<Grant>,
    /// The compositors it runs under: what it tests exists only there.
    pub(super) compositors: &'static [Compositor],
    /// Whether it runs again in the session at scale 2.
    pub(super) scaled: bool,
    /// The libtest arguments it runs with.
    pub(super) filter: &'static [&'static str],
}

/// The module of the window-management tests that ends the session, so
/// runs last, on its own.
pub(super) const SESSION_END: &str = "session_end::";

/// The binaries, in the order they run.
pub(super) const SUITES: [Suite; 8] = [
    Suite {
        target: Target {
            package: WINDOW_MANAGEMENT,
            test: "compositor",
        },
        grant: Some(Grant {
            id: "org.deskrinch.WindowManagementTests",
            name: "deskrinch window-management tests",
            wayland: &[deskrinch_window_management::KWIN_INTERFACE],
            dbus: &[],
        }),
        compositors: ALL,
        scaled: false,
        filter: &["--skip", SESSION_END],
    },
    Suite {
        target: Target {
            package: WINDOW_MANAGEMENT,
            test: "kwin_not_granted",
        },
        grant: None,
        compositors: KWIN,
        scaled: false,
        filter: &[],
    },
    Suite {
        target: Target {
            package: SCREEN,
            test: "compositor",
        },
        grant: Some(Grant {
            id: "org.deskrinch.TestScreenTests",
            name: "deskrinch screen-capture tests",
            wayland: &[],
            dbus: &[deskrinch_test_screen::KWIN_SCREENSHOT_INTERFACE],
        }),
        compositors: ALL,
        scaled: true,
        filter: &[],
    },
    Suite {
        target: Target {
            package: SCREEN,
            test: "wlroots",
        },
        grant: None,
        compositors: WLROOTS,
        scaled: true,
        filter: &[],
    },
    Suite {
        target: Target {
            package: SCREEN,
            test: "kwin_not_granted",
        },
        grant: None,
        compositors: KWIN,
        scaled: false,
        filter: &[],
    },
    // The input device, typing into test windows: under KWin through its
    // restricted fake input.
    Suite {
        target: Target {
            package: INPUT,
            test: "compositor",
        },
        grant: Some(Grant {
            id: "org.deskrinch.TestInputTests",
            name: "deskrinch input tests",
            wayland: &[deskrinch_test_input::KWIN_FAKE_INPUT_INTERFACE],
            dbus: &[],
        }),
        compositors: ALL,
        scaled: false,
        filter: &[],
    },
    // The launcher, its real binary: its process, and keys typed into it
    // with what it draws.
    Suite {
        target: Target {
            package: LAUNCHER,
            test: "compositor",
        },
        grant: Some(Grant {
            id: "org.deskrinch.LauncherTests",
            name: "deskrinch launcher tests",
            wayland: &[deskrinch_test_input::KWIN_FAKE_INPUT_INTERFACE],
            dbus: &[deskrinch_test_screen::KWIN_SCREENSHOT_INTERFACE],
        }),
        compositors: ALL,
        scaled: false,
        filter: &[],
    },
    // Last: it ends the session.
    Suite {
        target: Target {
            package: WINDOW_MANAGEMENT,
            test: "compositor",
        },
        grant: None,
        compositors: ALL,
        scaled: false,
        filter: &[SESSION_END],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_ends_last() {
        let last = SUITES.last().unwrap();
        assert_eq!(last.filter, [SESSION_END]);
        assert!(last.compositors == ALL && !last.scaled);
    }

    #[test]
    fn each_binary_is_granted_at_most_once() {
        for (index, suite) in SUITES.iter().enumerate() {
            let granted_again = SUITES[index + 1..]
                .iter()
                .any(|later| later.target == suite.target && later.grant.is_some());
            assert!(
                !(suite.grant.is_some() && granted_again),
                "{:?}",
                suite.target
            );
        }
    }
}

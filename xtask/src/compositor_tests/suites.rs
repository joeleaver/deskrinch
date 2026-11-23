//! The test binaries `cargo xtask compositor-tests` runs, and where: the
//! table of rows, each a [`Suite`] (`super::suite`).

use super::build::{Backend, Target};
pub(super) use super::passes::Pass;
pub(super) use super::suite::{Grant, SESSION_END, Suite};
use crate::session::Compositor;

const WINDOW_MANAGEMENT: &str = "deskrinch-window-management";
const SCREEN: &str = "deskrinch-test-screen";
const KWIN_VIEW: &str = "deskrinch-test-kwin";
const TEST_CLIENT: &str = "deskrinch-test-client";
const INPUT: &str = "deskrinch-test-input";
const LAUNCHER: &str = "deskrinch-launcher";
const NOTIFICATIONS: &str = "deskrinch-notifications";
const PANEL: &str = "deskrinch-panel";
const WALLPAPER: &str = "deskrinch-wallpaper";
const WAYLAND_CLIENT: &str = "deskrinch-wayland-client";
const WORKSPACES: &str = "deskrinch-workspaces";
const XTASK: &str = "xtask";

const ALL: &[Compositor] = &[Compositor::Kwin, Compositor::Sway, Compositor::Labwc];
const KWIN: &[Compositor] = &[Compositor::Kwin];
/// xtask's input client test, whose suite grants xtask's own binary too.
pub(super) const INPUT_CLIENT: Target = Target {
    package: XTASK,
    test: "input_client",
    backend: Backend::Rust,
};
const WLROOTS: &[Compositor] = &[Compositor::Sway, Compositor::Labwc];
const LABWC: &[Compositor] = &[Compositor::Labwc];

const PLAIN: &[Pass] = &[Pass::Plain];

/// The test client's tests of [`Pass::UhdBesideFullHd`]'s outputs: run by
/// that pass's row alone.
const UHD_BESIDE_FULL_HD_TESTS: &str = "uhd_beside_full_hd::";
/// The test client's window closed and the next opened at once, four
/// thousand times: run by its own row, under labwc alone.
const CLOSE_THEN_OPEN_TESTS: &str = "close_then_open::";

/// The workspace tests that need a compositor with no workspace protocol
/// (sway), and those that change KWin's desktops by KWin's own interface.
const UNSUPPORTED: &str = "unsupported::";
const KWIN_DESKTOPS: &str = "desktops::";
/// The workspace tests that share a connection with window management.
const SHARED: &str = "shared::";

/// The binaries, in the order they run.
pub(super) const SUITES: [Suite; 26] = [
    // The test client's output changes, first: that every change to the
    // session's outputs is put back exactly, which the suites after it
    // rely on.
    Suite {
        target: Target {
            package: TEST_CLIENT,
            test: "compositor",
            backend: Backend::Rust,
        },
        grant: None,
        compositors: ALL,
        passes: PLAIN,
        renders: false,
        programs: &[],
        filter: &[
            "--skip",
            UHD_BESIDE_FULL_HD_TESTS,
            "--skip",
            CLOSE_THEN_OPEN_TESTS,
        ],
    },
    // The same binary's window closed and the next opened at once, under
    // labwc alone. The race it guards is the client's own (a configure
    // handled while the test drops the window), so one compositor's timing
    // guards it: labwc meets it most often (about one window in seven
    // hundred without the fix; sway and KWin more rarely), and its job has
    // the most room for four thousand windows (about 25 s here).
    Suite {
        target: Target {
            package: TEST_CLIENT,
            test: "compositor",
            backend: Backend::Rust,
        },
        grant: None,
        compositors: LABWC,
        passes: PLAIN,
        renders: false,
        programs: &[],
        filter: &[CLOSE_THEN_OPEN_TESTS, "--skip", UHD_BESIDE_FULL_HD_TESTS],
    },
    // The same binary's tests of a 3840x2160 output beside a 1920x1080
    // one, on those outputs alone.
    Suite {
        target: Target {
            package: TEST_CLIENT,
            test: "compositor",
            backend: Backend::Rust,
        },
        grant: None,
        compositors: ALL,
        passes: &[Pass::UhdBesideFullHd],
        renders: false,
        programs: &[],
        filter: &[UHD_BESIDE_FULL_HD_TESTS],
    },
    Suite {
        target: Target {
            package: WINDOW_MANAGEMENT,
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
    },
    Suite {
        target: Target {
            package: WINDOW_MANAGEMENT,
            test: "kwin_not_granted",
            backend: Backend::Rust,
        },
        grant: None,
        compositors: KWIN,
        passes: PLAIN,
        renders: false,
        programs: &[],
        filter: &[],
    },
    // What KWin itself holds of the test client's windows (their types,
    // above all), read through a KWin script, and what it composites with:
    // KWin only. Its output is shown, for the compositing type it prints.
    Suite {
        target: Target {
            package: KWIN_VIEW,
            test: "compositor",
            backend: Backend::Rust,
        },
        grant: None,
        compositors: KWIN,
        passes: PLAIN,
        renders: false,
        programs: &[],
        filter: &["--show-output"],
    },
    Suite {
        target: Target {
            package: SCREEN,
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
    },
    Suite {
        target: Target {
            package: SCREEN,
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
    },
    Suite {
        target: Target {
            package: SCREEN,
            test: "kwin_not_granted",
            backend: Backend::Rust,
        },
        grant: None,
        compositors: KWIN,
        passes: PLAIN,
        renders: false,
        programs: &[],
        filter: &[],
    },
    // The input devices, typing into test windows and pointing at them:
    // under KWin through its restricted fake input. Every pass, since where
    // the pointer lands depends on the outputs' scale and transform.
    Suite {
        target: Target {
            package: INPUT,
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
    },
    // The launcher, its real binary: its process, and keys typed into it
    // with what it draws; once per renderer of the run.
    Suite {
        target: Target {
            package: LAUNCHER,
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
    },
    // The notification daemon's real binary, sent notifications on the
    // session's bus, its popups checked by the screen and clicked with the
    // pointer. Not in the flipped pass, for the panel's reason: a turned
    // output already moves the corner the column is anchored to.
    Suite {
        target: Target {
            package: NOTIFICATIONS,
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
    },
    // The panel's real binary, checked by the screen: it captures under
    // KWin. Not in the flipped pass: a full-width band along an edge is the
    // same mirrored, so that pass adds nothing a turned output does not.
    Suite {
        target: Target {
            package: PANEL,
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
        filter: &[],
    },
    // The wallpaper's real binary, checked by the screen on every output; the
    // panel, declared as its program, built with it and handed to its tests,
    // is started over it. Not in the flipped pass, for the panel's reason: a
    // full output of one colour is the same mirrored.
    Suite {
        target: Target {
            package: WALLPAPER,
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
    },
    // The workspaces. KWin offers its virtual-desktop protocol ungranted, so
    // the grant lists only what the tests also use: window management on a
    // shared connection, and screenshots. labwc's workspaces are fixed by
    // its configuration (four), so KWin's `desktops::` tests do not run
    // there; sway offers no workspace protocol, which `unsupported::`
    // checks, alone.
    Suite {
        target: Target {
            package: WORKSPACES,
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
    },
    Suite {
        target: Target {
            package: WORKSPACES,
            test: "compositor",
            backend: Backend::Rust,
        },
        grant: None,
        compositors: &[Compositor::Labwc],
        passes: PLAIN,
        renders: false,
        programs: &[],
        filter: &["--skip", UNSUPPORTED, "--skip", KWIN_DESKTOPS],
    },
    Suite {
        target: Target {
            package: WORKSPACES,
            test: "compositor",
            backend: Backend::Rust,
        },
        grant: None,
        compositors: &[Compositor::Sway],
        passes: PLAIN,
        renders: false,
        programs: &[],
        filter: &[UNSUPPORTED],
    },
    // The shared connection's one thread: states attached and detached from
    // several threads at once, and how the thread and its states end.
    Suite {
        target: Target {
            package: WAYLAND_CLIENT,
            test: "compositor",
            backend: Backend::Rust,
        },
        grant: None,
        compositors: ALL,
        passes: PLAIN,
        renders: false,
        programs: &[],
        filter: &[],
    },
    // `cargo xtask desktop`, nested: its window opens in this session, which
    // the binary closes through window management and types the launcher's
    // chord into. Its output is shown, for what the edge test found inside
    // the desktop (the desktop's own KWin's compositing type among it).
    Suite {
        target: Target {
            package: XTASK,
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
        filter: &["--show-output"],
    },
    // `cargo xtask input`, measure's input client: its lines reach a window
    // of the test's own. Under KWin the client, xtask's own binary, is
    // granted fake input with the suite (`granted.rs`).
    Suite {
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
    },
    // Each shell component's end when its display goes away, in a session
    // of its own nested in this one, whose compositor the test kills.
    Suite {
        target: Target {
            package: XTASK,
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
    },
    // Each shell component's end when it is asked to stop (`SIGTERM`,
    // `SIGINT`), in a session of its own nested in this one.
    Suite {
        target: Target {
            package: XTASK,
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
    },
    // The two suites that exercise the shared connection, again on
    // libwayland: what every binary that links rinch gets (winit selects
    // `wayland-client`'s `system` backend), while these crates alone get the
    // Rust one. Window management's session-ending test runs once, last, on
    // the Rust backend: a session ends only once.
    Suite {
        target: Target {
            package: WAYLAND_CLIENT,
            test: "compositor",
            backend: Backend::System,
        },
        grant: None,
        compositors: ALL,
        passes: PLAIN,
        renders: false,
        programs: &[],
        filter: &[],
    },
    Suite {
        target: Target {
            package: WINDOW_MANAGEMENT,
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
    },
    // The workspaces' shared-connection tests on libwayland too: they share
    // one connection with window management.
    Suite {
        target: Target {
            package: WORKSPACES,
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
    },
    // Last: it ends the session.
    Suite {
        target: Target {
            package: WINDOW_MANAGEMENT,
            test: "compositor",
            backend: Backend::Rust,
        },
        grant: None,
        compositors: ALL,
        passes: PLAIN,
        renders: false,
        programs: &[],
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
        assert!(last.compositors == ALL && last.passes == PLAIN);
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

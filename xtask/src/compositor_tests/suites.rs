//! The test binaries `cargo xtask compositor-tests` runs, and where.

use deskrinch_test_wayland::Transform;

use super::build::{Backend, Bin, Target};
use crate::session::{Compositor, Outputs, Scale};

const WINDOW_MANAGEMENT: &str = "deskrinch-window-management";
const SCREEN: &str = "deskrinch-test-screen";
const INPUT: &str = "deskrinch-test-input";
const LAUNCHER: &str = "deskrinch-launcher";
const PANEL: &str = "deskrinch-panel";
const WAYLAND_CLIENT: &str = "deskrinch-wayland-client";

const ALL: &[Compositor] = &[Compositor::Kwin, Compositor::Sway, Compositor::Labwc];
const KWIN: &[Compositor] = &[Compositor::Kwin];
const WLROOTS: &[Compositor] = &[Compositor::Sway, Compositor::Labwc];

/// How many outputs the sessions have, side by side: the window-management
/// tests move windows between two, and the screen tests tell them apart.
const OUTPUTS: u32 = 2;

/// A session the suites run in, by how its outputs are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pass {
    /// Scale 1, upright: every suite.
    Plain,
    /// Scale 2, for the suites that check scaling.
    Scaled,
    /// Turned a quarter, for the suites that check transforms.
    Rotated,
    /// Mirrored and turned a quarter (`flipped-90`): a flip as well as a
    /// swap of the axes, for the suites that check transforms.
    Flipped,
}

impl Pass {
    /// Every pass, in the order they run.
    pub(super) const ALL: [Self; 4] = [Self::Plain, Self::Scaled, Self::Rotated, Self::Flipped];

    /// The session's outputs in this pass.
    pub(super) fn outputs(self) -> Outputs {
        let plain = Outputs::plain(OUTPUTS);
        match self {
            Self::Plain => plain,
            Self::Scaled => Outputs {
                scale: Scale::TWO,
                ..plain
            },
            Self::Rotated => Outputs {
                transform: Transform::Rotated90.into(),
                ..plain
            },
            Self::Flipped => Outputs {
                transform: Transform::Flipped90.into(),
                ..plain
            },
        }
    }
}

const PLAIN: &[Pass] = &[Pass::Plain];

/// What KWin grants a test binary.
#[derive(Debug)]
pub(super) struct Grant {
    /// The desktop file id.
    pub(super) id: &'static str,
    pub(super) name: &'static str,
    pub(super) wayland: &'static [&'static str],
    pub(super) dbus: &'static [&'static str],
}

/// A program a suite's tests start, and what KWin grants it: one desktop
/// file for its binary, as for a test binary.
#[derive(Debug)]
pub(super) struct Program {
    pub(super) bin: Bin,
    pub(super) grant: Grant,
}

/// A test binary and how it runs.
#[derive(Debug)]
pub(super) struct Suite {
    pub(super) target: Target,
    /// What KWin grants it; nothing for a binary that checks a refusal.
    pub(super) grant: Option<Grant>,
    /// The programs its tests start that KWin must grant something (the
    /// panel, for its taskbar's window list).
    pub(super) programs: &'static [Program],
    /// The compositors it runs under: what it tests exists only there.
    pub(super) compositors: &'static [Compositor],
    /// The sessions it runs in, by how their outputs are.
    pub(super) passes: &'static [Pass],
    /// Whether it draws with rinch, and so runs once per renderer the run
    /// tests ([`super::renderers`]), told which in
    /// `DESKRINCH_TEST_RENDERER`.
    pub(super) renders: bool,
    /// The libtest arguments it runs with.
    pub(super) filter: &'static [&'static str],
}

/// The module of the window-management tests that ends the session, so
/// runs last, on its own.
pub(super) const SESSION_END: &str = "session_end::";

/// The binaries, in the order they run.
pub(super) const SUITES: [Suite; 12] = [
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
        programs: &[],
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
        // The panel's taskbar follows the window list.
        programs: &[Program {
            bin: Bin {
                package: PANEL,
                bin: "deskrinch-panel",
            },
            grant: Grant {
                id: "org.deskrinch.PanelUnderTest",
                name: "deskrinch panel under test",
                wayland: &[deskrinch_window_management::KWIN_INTERFACE],
                dbus: &[],
            },
        }],
        filter: &[],
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

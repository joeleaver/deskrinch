//! The test binaries `cargo xtask compositor-tests` runs, and where.

use deskrinch_test_wayland::Transform;

use super::build::Target;
use crate::session::{Compositor, Outputs, Scale};

const WINDOW_MANAGEMENT: &str = "deskrinch-window-management";
const SCREEN: &str = "deskrinch-test-screen";
const LAUNCHER: &str = "deskrinch-launcher";

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

/// A test binary and how it runs.
#[derive(Debug)]
pub(super) struct Suite {
    pub(super) target: Target,
    /// What KWin grants it; nothing for a binary that checks a refusal.
    pub(super) grant: Option<Grant>,
    /// The compositors it runs under: what it tests exists only there.
    pub(super) compositors: &'static [Compositor],
    /// The sessions it runs in, by how their outputs are.
    pub(super) passes: &'static [Pass],
    /// The libtest arguments it runs with.
    pub(super) filter: &'static [&'static str],
}

/// The module of the window-management tests that ends the session, so
/// runs last, on its own.
pub(super) const SESSION_END: &str = "session_end::";

/// The binaries, in the order they run.
pub(super) const SUITES: [Suite; 7] = [
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
        passes: PLAIN,
        filter: &["--skip", SESSION_END],
    },
    Suite {
        target: Target {
            package: WINDOW_MANAGEMENT,
            test: "kwin_not_granted",
        },
        grant: None,
        compositors: KWIN,
        passes: PLAIN,
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
        passes: &Pass::ALL,
        filter: &[],
    },
    Suite {
        target: Target {
            package: SCREEN,
            test: "wlroots",
        },
        grant: None,
        compositors: WLROOTS,
        // It rotates an output itself.
        passes: &[Pass::Plain, Pass::Scaled],
        filter: &[],
    },
    Suite {
        target: Target {
            package: SCREEN,
            test: "kwin_not_granted",
        },
        grant: None,
        compositors: KWIN,
        passes: PLAIN,
        filter: &[],
    },
    // The launcher's process, its real binary: no restricted interface.
    Suite {
        target: Target {
            package: LAUNCHER,
            test: "process",
        },
        grant: None,
        compositors: ALL,
        passes: PLAIN,
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
        passes: PLAIN,
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

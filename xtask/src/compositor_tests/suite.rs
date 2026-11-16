//! What a row of the suites table ([`super::suites::SUITES`]) is: a test
//! binary, what KWin grants it, and the compositors and passes it runs in.

use deskrinch_shell::Component;
use deskrinch_test_wayland::Transform;

use super::build::Target;
use crate::session::{Compositor, Outputs, Scale};

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
    /// Whether it draws with rinch, and so runs once per renderer the run
    /// tests ([`super::renderers`]), told which in
    /// `DESKRINCH_TEST_RENDERER`.
    pub(super) renders: bool,
    /// Every shell component its tests start, its own crate's included:
    /// handed to its tests by path, the only way they start one
    /// ([`deskrinch_test_component::programs`], where the rule is).
    pub(super) programs: &'static [&'static Component],
    /// The libtest arguments it runs with.
    pub(super) filter: &'static [&'static str],
}

impl Suite {
    /// Whether it is the row that ends the session (the window-management
    /// tests of [`SESSION_END`]), after which nothing of it is left.
    pub(super) fn ends_the_session(&self) -> bool {
        self.filter == [SESSION_END]
    }
}

/// The module of the window-management tests that ends the session, so
/// runs last, on its own.
pub(super) const SESSION_END: &str = "session_end::";

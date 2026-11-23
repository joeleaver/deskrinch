//! What a row of the suites table ([`super::suites::SUITES`]) is: a test
//! binary, what KWin grants it, and the compositors and passes
//! ([`super::passes`]) it runs in.

use deskrinch_shell::Component;

use super::build::Target;
use super::passes::Pass;
use crate::session::Compositor;

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
    /// Which of its tests it runs, as libtest reads its filter: the words
    /// a test's name contains, and `--skip` before each word a test's name
    /// must not contain. Nothing else: an option here would be read as a
    /// name by [`super::selection`], so a test's output shown is
    /// [`Suite::output`].
    pub(super) filter: &'static [&'static str],
    /// Whether its tests' output is shown, passing or not.
    pub(super) output: Output,
}

/// Whether a suite's tests' output is shown when they pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Output {
    /// Captured by libtest, shown only for a test that fails.
    Captured,
    /// Shown for every test (`--show-output`): for what a test prints that
    /// a reader of the run wants, such as what KWin composites with.
    Shown,
}

impl Output {
    /// The libtest arguments that make it so, after the suite's filter or
    /// the test asked for.
    pub(super) fn args(self) -> &'static [&'static str] {
        match self {
            Self::Captured => &[],
            Self::Shown => &["--show-output"],
        }
    }
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

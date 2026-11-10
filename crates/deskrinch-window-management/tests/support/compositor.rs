//! Which nested compositor the tests run in.

/// The nested compositor `cargo xtask compositor-tests` started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Compositor {
    Kwin,
    Sway,
    Labwc,
}

impl Compositor {
    /// The compositor the tests run in. Panics unless the tests were started
    /// by `cargo xtask compositor-tests`, which sets
    /// `DESKRINCH_TEST_COMPOSITOR`: run directly, they would open, activate
    /// and close windows in the real session.
    pub(crate) fn current() -> Self {
        match std::env::var("DESKRINCH_TEST_COMPOSITOR").as_deref() {
            Ok("kwin") => Self::Kwin,
            Ok("sway") => Self::Sway,
            Ok("labwc") => Self::Labwc,
            _ => panic!(
                "the compositor tests run in a nested compositor: \
                 use `cargo xtask compositor-tests`"
            ),
        }
    }
}

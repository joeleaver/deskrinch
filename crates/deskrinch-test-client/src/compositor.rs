//! Which nested compositor the tests run in.

/// The nested compositor `cargo xtask compositor-tests` started, as it
/// tells the tests in `DESKRINCH_TEST_COMPOSITOR`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compositor {
    /// KWin.
    Kwin,
    /// sway.
    Sway,
    /// labwc.
    Labwc,
}

impl Compositor {
    /// The compositor the tests run in.
    ///
    /// # Panics
    ///
    /// Unless the tests were started by `cargo xtask compositor-tests`,
    /// which sets `DESKRINCH_TEST_COMPOSITOR`: run directly, they would
    /// open, activate, close and photograph windows in the real session.
    #[expect(
        clippy::must_use_candidate,
        reason = "a test calls it for its check alone, before touching the compositor"
    )]
    pub fn current() -> Self {
        Self::nested().unwrap_or_else(|| {
            panic!(
                "the compositor tests run in a nested compositor: \
                 use `cargo xtask compositor-tests`"
            )
        })
    }

    /// The nested compositor this process runs in, as
    /// `DESKRINCH_TEST_COMPOSITOR` says (`cargo xtask compositor-tests` and
    /// `cargo xtask session` set it); `None` outside one. For a program
    /// that refuses with an error of its own rather than panicking.
    #[must_use]
    pub fn nested() -> Option<Self> {
        match std::env::var("DESKRINCH_TEST_COMPOSITOR").as_deref() {
            Ok("kwin") => Some(Self::Kwin),
            Ok("sway") => Some(Self::Sway),
            Ok("labwc") => Some(Self::Labwc),
            _ => None,
        }
    }
}

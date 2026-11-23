//! Which nested compositor the tests run in.

use deskrinch_test_session::{Environment, check};

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
    /// Unless the tests were started inside an isolated session
    /// (`cargo xtask compositor-tests`, [`nested`](Self::nested)): run
    /// directly, they would open, activate, close and photograph windows
    /// in the real session.
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

    /// The nested compositor this process runs in, inside an isolated
    /// session (`cargo xtask compositor-tests` and `cargo xtask session`
    /// make them): the session's own check ([`deskrinch_test_session::check`])
    /// passes, and its compositor is one of these. `None` anywhere else. For
    /// a program that refuses with an error of its own rather than
    /// panicking.
    #[must_use]
    pub fn nested() -> Option<Self> {
        Self::in_environment(&Environment::read())
    }

    /// [`nested`](Self::nested), for `environment`.
    fn in_environment(environment: &Environment) -> Option<Self> {
        match check(environment).ok()?.compositor.as_str() {
            "kwin" => Some(Self::Kwin),
            "sway" => Some(Self::Sway),
            "labwc" => Some(Self::Labwc),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The compositor's name alone, the one variable `nested` once read, is
    /// not enough: it goes through the session's check, which this
    /// environment fails (no session root, and the real session's bus).
    #[test]
    fn the_compositor_s_name_alone_is_not_a_session() {
        let named = Environment {
            compositor: Some("kwin".into()),
            session_dir: None,
            bus: Some("unix:path=/run/user/1000/bus".into()),
        };
        assert_eq!(Compositor::in_environment(&named), None);
        assert_eq!(Compositor::in_environment(&Environment::default()), None);
    }
}

//! Which nested compositor the tests run in.

use std::fmt;

use deskrinch_test_session::{Environment, NotIsolated, check};

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
        Self::in_session().unwrap_or_else(|why| {
            panic!(
                "the compositor tests run in a nested compositor started by \
                 `cargo xtask compositor-tests`: {why}"
            )
        })
    }

    /// The nested compositor this process runs in, inside an isolated
    /// session (`cargo xtask compositor-tests` and `cargo xtask session`
    /// make them): the session's own check ([`deskrinch_test_session::check`])
    /// passes, and its compositor is one of these. Anywhere else, why not,
    /// for a refusal to say.
    ///
    /// # Errors
    ///
    /// [`NotNested`], with the check's own reason.
    pub fn in_session() -> Result<Self, NotNested> {
        Self::in_environment(&Environment::read())
    }

    /// [`in_session`](Self::in_session), without the reason: `None` outside
    /// a nested compositor this crate knows.
    #[must_use]
    pub fn nested() -> Option<Self> {
        Self::in_session().ok()
    }

    /// [`in_session`](Self::in_session), for `environment`.
    fn in_environment(environment: &Environment) -> Result<Self, NotNested> {
        let isolated = check(environment).map_err(NotNested::NotIsolated)?;
        match isolated.compositor.as_str() {
            "kwin" => Ok(Self::Kwin),
            "sway" => Ok(Self::Sway),
            "labwc" => Ok(Self::Labwc),
            _ => Err(NotNested::Unknown {
                compositor: isolated.compositor,
            }),
        }
    }
}

/// Why this process is not in a nested compositor this crate knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotNested {
    /// It is not inside an isolated session at all.
    NotIsolated(NotIsolated),
    /// Its session's compositor is not one of [`Compositor`]'s.
    Unknown {
        /// The compositor, as the session names it.
        compositor: String,
    },
}

impl fmt::Display for NotNested {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotIsolated(why) => why.fmt(f),
            Self::Unknown { compositor } => write!(
                f,
                "the session's compositor is {compositor:?}, which is not KWin, sway or labwc"
            ),
        }
    }
}

impl std::error::Error for NotNested {}

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
        assert_eq!(
            Compositor::in_environment(&named),
            Err(NotNested::NotIsolated(NotIsolated::NoSessionDir))
        );
        assert_eq!(
            Compositor::in_environment(&Environment::default()),
            Err(NotNested::NotIsolated(NotIsolated::NoCompositor))
        );
    }
}

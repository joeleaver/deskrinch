//! The refusal outside an isolated KWin session, before any connection.
//!
//! Every public question goes through [`guarded`]: the check's result and
//! the question are passed in, so that the refusal is tested in-process
//! with a question that must not be asked, and no bus anywhere. That the
//! public calls pass the session's real check is tested in a child of the
//! test binary outside any session (`tests.rs`).

use deskrinch_test_session::{Environment, check};

use crate::Error;

/// The compositor's name an isolated KWin session gives.
const KWIN: &str = "kwin";

/// Fails unless `environment` is that of a process inside an isolated
/// session whose compositor is KWin.
pub(crate) fn require_kwin(environment: &Environment) -> Result<(), Error> {
    let isolated = check(environment).map_err(Error::NotIsolated)?;
    if isolated.compositor == KWIN {
        Ok(())
    } else {
        Err(Error::NotKwin {
            compositor: isolated.compositor,
        })
    }
}

/// [`require_kwin`] for this process.
pub(crate) fn require_kwin_here() -> Result<(), Error> {
    require_kwin(&Environment::read())
}

/// `ask`'s answer, asked only once `allowed` (the check's result) is `Ok`.
pub(crate) fn guarded<T>(
    allowed: Result<(), Error>,
    ask: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    allowed?;
    ask()
}

#[cfg(test)]
mod tests {
    use deskrinch_test_session::NotIsolated;

    use super::*;

    /// The question of a refusal test, which must not be asked.
    fn must_not_ask() -> Result<(), Error> {
        panic!("asked KWin, although the process runs in no isolated KWin session");
    }

    #[test]
    fn outside_a_session_nothing_is_asked() {
        let refusal = guarded(require_kwin(&Environment::default()), must_not_ask).unwrap_err();
        assert!(
            matches!(refusal, Error::NotIsolated(NotIsolated::NoCompositor)),
            "{refusal:?}"
        );
    }

    #[test]
    fn inside_an_isolated_kwin_session_the_question_is_asked() {
        assert_eq!(guarded(Ok(()), || Ok(7)).unwrap(), 7);
    }

    #[test]
    fn outside_a_session_it_refuses_saying_why() {
        let refusal = require_kwin(&Environment::default()).unwrap_err();
        assert!(
            matches!(refusal, Error::NotIsolated(NotIsolated::NoCompositor)),
            "{refusal:?}"
        );
        assert!(
            refusal
                .to_string()
                .starts_with("refusing to ask KWin: not inside an isolated session"),
            "{refusal}"
        );
    }

    /// The owner's own session, with the compositor's name set by hand: the
    /// name alone is not a session.
    #[test]
    fn the_real_session_with_kwin_named_is_refused() {
        let real = Environment {
            compositor: Some("kwin".into()),
            session_dir: None,
            bus: Some("unix:path=/run/user/1000/bus".into()),
        };
        assert!(matches!(
            require_kwin(&real),
            Err(Error::NotIsolated(NotIsolated::NoSessionDir))
        ));
    }

    /// A session of ours under sway: isolated, and still refused.
    #[test]
    fn another_compositor_s_session_is_refused() {
        let root = tempfile::Builder::new()
            .prefix(deskrinch_test_session::root::PREFIX)
            .tempdir_in(deskrinch_test_session::root::PARENT)
            .unwrap();
        std::fs::write(root.path().join(deskrinch_test_session::root::MARKER), "").unwrap();
        let sway = Environment {
            compositor: Some("sway".into()),
            session_dir: Some(root.path().as_os_str().to_owned()),
            bus: Some(
                deskrinch_test_session::bus::listen_address(
                    &deskrinch_test_session::bus::socket_name(1, 0),
                )
                .into(),
            ),
        };
        let refusal = require_kwin(&sway).unwrap_err();
        assert!(
            matches!(&refusal, Error::NotKwin { compositor } if compositor == "sway"),
            "{refusal:?}"
        );
    }
}

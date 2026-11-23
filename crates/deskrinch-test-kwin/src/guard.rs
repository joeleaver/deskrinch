//! The refusal outside an isolated KWin session, before any connection.
//!
//! The guard is a matter of types: [`require_kwin`] is the only maker of
//! [`InKwinSession`], the proof that the check passed, and everything that
//! touches a bus takes one: the questions, the making of a connection
//! ([`session_bus`]) and the loading of a script
//! ([`Loaded::run`](crate::script::Loaded::run)). A public call that
//! forgot the guard does not compile.
//!
//! Every public question goes through [`guarded`]: the check's result and
//! the question are passed in, so that the refusal is tested in-process
//! with a question that must not be asked, and no bus anywhere. That the
//! public calls pass the session's real check, not another way to a proof,
//! is tested in a child of the test binary outside any session
//! (`wiring_tests`).

use deskrinch_test_session::{Environment, KWIN, check};

use crate::Error;

/// Proof that the session's check passed: only [`require_kwin`] makes one.
#[derive(Debug)]
pub(crate) struct InKwinSession {
    _checked: (),
}

/// The proof, unless `environment` is not that of a process inside an
/// isolated session whose compositor is KWin.
pub(crate) fn require_kwin(environment: &Environment) -> Result<InKwinSession, Error> {
    let isolated = check(environment).map_err(Error::NotIsolated)?;
    if isolated.compositor == KWIN {
        Ok(InKwinSession { _checked: () })
    } else {
        Err(Error::NotKwin {
            compositor: isolated.compositor,
        })
    }
}

/// [`require_kwin`] for this process.
pub(crate) fn require_kwin_here() -> Result<InKwinSession, Error> {
    require_kwin(&Environment::read())
}

/// `ask`'s answer, asked with the proof only once `allowed` (the check's
/// result) holds one.
pub(crate) fn guarded<T>(
    allowed: Result<InKwinSession, Error>,
    ask: impl FnOnce(&InKwinSession) -> Result<T, Error>,
) -> Result<T, Error> {
    let proof = allowed?;
    ask(&proof)
}

/// The session's bus, to connect to, in the session `_proof` checked.
pub(crate) fn session_bus(_proof: &InKwinSession) -> zbus::connection::Builder<'static> {
    zbus::connection::Builder::session()
}

#[cfg(test)]
mod tests {
    use deskrinch_test_session::NotIsolated;

    use super::*;

    /// The question of a refusal test, which must not be asked.
    fn must_not_ask(_: &InKwinSession) -> Result<(), Error> {
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
        assert_eq!(
            guarded(Ok(InKwinSession { _checked: () }), |_| Ok(7)).unwrap(),
            7
        );
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
            compositor: Some(KWIN.into()),
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
        deskrinch_test_session::root::mark(root.path()).unwrap();
        let sway = Environment {
            compositor: Some(deskrinch_test_session::SWAY.into()),
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
            matches!(&refusal, Error::NotKwin { compositor } if compositor == deskrinch_test_session::SWAY),
            "{refusal:?}"
        );
    }
}

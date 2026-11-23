//! The refusal outside a nested KWin session, in two forms that are
//! harmless when the refusal fails (mutants of a guard are run only where
//! the guard's failure is harmless, `deskrinch-test-session`'s doc):
//!
//! - the logic, in-process: the check's result and the connector are
//!   passed in ([`checked`]), and the connector is a closure that must not
//!   be called, so no bus is reached anywhere;
//! - the wiring (that the public calls pass the session's real check), in
//!   a child: this test binary runs itself for one ignored inner test,
//!   [`outside`] any session, where a call that was not refused finds a
//!   bus address that leads nowhere.

use std::process::{Command, Stdio};

use deskrinch_test_client::{Compositor, NotNested};
use deskrinch_test_dbus::session::outside;
use deskrinch_test_session::NotIsolated;

use super::{checked, desktops, require_kwin, set_up};

/// The connector of a refusal test, which must not be called.
fn must_not_connect() {
    panic!("connected, although the process runs in no nested KWin");
}

#[test]
#[should_panic(expected = "this process runs in no nested session (not inside an isolated session")]
fn outside_a_session_it_refuses_before_connecting() {
    checked(
        Err(NotNested::NotIsolated(NotIsolated::NoCompositor)),
        must_not_connect,
    );
}

#[test]
#[should_panic(expected = "a nested sway, which has no KWin")]
fn sway_is_refused_before_connecting() {
    checked(Ok(Compositor::Sway), must_not_connect);
}

#[test]
#[should_panic(expected = "a nested labwc, which has no KWin")]
fn labwc_is_refused() {
    require_kwin(Ok(Compositor::Labwc));
}

#[test]
fn kwin_is_let_through_to_connect() {
    assert_eq!(checked(Ok(Compositor::Kwin), || 7), 7);
}

#[test]
#[should_panic(expected = "no nested session (the session's compositor is \"weston\"")]
fn an_unknown_compositor_is_refused_with_the_reason() {
    require_kwin(Err(NotNested::Unknown {
        compositor: "weston".to_owned(),
    }));
}

/// Set by the outer tests for the inner ones, which do nothing without it:
/// they are run only in the child the outer test makes.
const INNER: &str = "DESKRINCH_TEST_DESKTOPS_INNER";

/// The inner test `name` run in a child of this test binary, outside any
/// session; what it said, having failed.
fn refused_in_a_child(name: &str) -> String {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args(["--ignored", "--exact", "--test-threads=1", name])
        .env(INNER, "1")
        .stdin(Stdio::null());
    let _root = outside(&mut child).unwrap();
    let output = child.output().unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("1 failed"), "{name} did not fail: {said}");
    said
}

#[test]
fn a_call_outside_a_nested_session_is_refused_before_any_bus() {
    let said = refused_in_a_child("tests::inner::lists_the_desktops");
    assert!(
        said.contains("this process runs in no nested session (not inside an isolated session"),
        "{said}"
    );
}

#[test]
fn setting_up_outside_a_nested_session_says_what_it_would_have_done() {
    let said = refused_in_a_child("tests::inner::sets_up_the_desktops");
    assert!(
        said.contains("the call would lay out, switch, add and remove the real session's desktops"),
        "{said}"
    );
}

/// The public calls, run only by the outer tests above in their child.
mod inner {
    use super::{INNER, desktops, set_up};

    #[test]
    #[ignore = "run by its outer test, in a child outside any session"]
    fn lists_the_desktops() {
        if std::env::var_os(INNER).is_some() {
            let _listed = desktops();
        }
    }

    #[test]
    #[ignore = "run by its outer test, in a child outside any session"]
    fn sets_up_the_desktops() {
        if std::env::var_os(INNER).is_some() {
            let _first = set_up(4);
        }
    }
}

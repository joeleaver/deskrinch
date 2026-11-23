//! That the handle's public makers read what they say, tested in a child:
//! this test binary runs itself for one ignored inner test, outside any
//! session (`deskrinch_test_dbus::session::outside`), where a maker that did
//! not refuse would hand out a handle to a bus address that leads nowhere.
//! The makers' logic is tested in-process in `kwin`.
//!
//! The address the child gives `Kwin::on` is the one `outside` gives it: a
//! `unix:path=` address, as the real session's is, under a temporary root
//! with nothing listening, so that a maker that took it would reach
//! nothing.

use std::process::{Command, Stdio};

use deskrinch_test_dbus::session::outside;

/// Set by the outer tests for the inner ones, which do nothing without it:
/// they are run only in the child the outer test makes.
const INNER: &str = "DESKRINCH_TEST_KWIN_INNER";

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
fn here_refuses_outside_a_session_before_any_bus() {
    let said = refused_in_a_child("wiring_tests::inner::asks_here");
    assert!(
        said.contains("refusing to ask KWin: not inside an isolated session"),
        "{said}"
    );
}

#[test]
fn on_refuses_an_address_not_ours_before_any_bus() {
    let said = refused_in_a_child("wiring_tests::inner::asks_on_the_session_s_address");
    assert!(said.contains("is not a private bus of ours"), "{said}");
}

/// The public makers, run only by the outer tests above in their child;
/// each asks a question with the handle, should it have been made.
mod inner {
    use deskrinch_test_session::BUS_VARIABLE;

    use super::INNER;
    use crate::Kwin;

    #[test]
    #[ignore = "run by its outer test, in a child outside any session"]
    fn asks_here() {
        if std::env::var_os(INNER).is_some()
            && let Err(why) = Kwin::here().and_then(|kwin| kwin.version())
        {
            panic!("{why}");
        }
    }

    #[test]
    #[ignore = "run by its outer test, in a child outside any session"]
    fn asks_on_the_session_s_address() {
        if std::env::var_os(INNER).is_some() {
            let address = std::env::var(BUS_VARIABLE).unwrap();
            if let Err(why) = Kwin::on(&address).and_then(|kwin| kwin.version()) {
                panic!("{why}");
            }
        }
    }
}

//! That the public calls pass the session's real check before any
//! connection, tested in a child: this test binary runs itself for one
//! ignored inner test, outside any session
//! (`deskrinch_test_dbus::session::outside`), where a call that was not
//! refused finds a bus address that leads nowhere. The refusal's logic is
//! tested in-process in `guard`.

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

/// What a refusal outside any session says.
const REFUSED: &str = "refusing to ask KWin: not inside an isolated session";

#[test]
fn reading_the_windows_outside_a_session_is_refused_before_any_bus() {
    let said = refused_in_a_child("wiring_tests::inner::reads_the_windows");
    assert!(said.contains(REFUSED), "{said}");
}

#[test]
fn reading_the_version_outside_a_session_is_refused_before_any_bus() {
    let said = refused_in_a_child("wiring_tests::inner::reads_the_version");
    assert!(said.contains(REFUSED), "{said}");
}

/// The public calls, run only by the outer tests above in their child.
mod inner {
    use super::INNER;

    #[test]
    #[ignore = "run by its outer test, in a child outside any session"]
    fn reads_the_windows() {
        if std::env::var_os(INNER).is_some()
            && let Err(why) = crate::windows()
        {
            panic!("{why}");
        }
    }

    #[test]
    #[ignore = "run by its outer test, in a child outside any session"]
    fn reads_the_version() {
        if std::env::var_os(INNER).is_some()
            && let Err(why) = crate::version()
        {
            panic!("{why}");
        }
    }
}

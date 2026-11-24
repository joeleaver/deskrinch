//! KWin's screenshot service is asked only on a bus proved ours: outside
//! any session, `ScreenShot2::connect` refuses before any connection. Shown
//! in a child of this test binary under `outside`, whose session bus
//! address leads nowhere, so that a connect that did not refuse would reach
//! nothing. (The proof's own makers are tested in `deskrinch-test-dbus`.)

use std::process::{Command, Stdio};

use deskrinch_test_dbus::session::outside;

/// Set by the outer test for the inner one, which does nothing without it.
const INNER: &str = "DESKRINCH_TEST_SCREEN_INNER";

#[test]
fn outside_a_session_the_screenshot_service_is_not_asked() {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--ignored",
            "--exact",
            "--test-threads=1",
            "kwin::refusal_tests::inner::connects_to_the_screenshot_service",
        ])
        .env(INNER, "1")
        .stdin(Stdio::null());
    let _root = outside(&mut child).unwrap();
    let output = child.output().unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("1 failed"), "it did not fail: {said}");
    assert!(
        said.contains(
            "refusing to ask for KWin's screenshot service: not inside an isolated session"
        ),
        "{said}"
    );
}

/// The connect, run only by the outer test above in its child.
mod inner {
    use deskrinch_test_wait::PATIENCE;

    use super::INNER;
    use crate::kwin::ScreenShot2;

    #[test]
    #[ignore = "run by its outer test, in a child outside any session"]
    fn connects_to_the_screenshot_service() {
        if std::env::var_os(INNER).is_some()
            && let Err(why) = ScreenShot2::connect(PATIENCE)
        {
            panic!("{why}");
        }
    }
}

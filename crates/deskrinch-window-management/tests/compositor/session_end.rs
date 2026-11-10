//! The compositor going away under a connected manager. This ends the
//! session, so `cargo xtask compositor-tests` runs it last and alone, and
//! tells it the session's process group.

use deskrinch_window_management::{Action, ActionError, EndReason, WindowManagement};
use rustix::process::{Pid, Signal, kill_process_group};

use crate::support::test_window::TestClient;
use crate::support::{Compositor, Watcher, unique_app_id};

/// The session's process group, from `DESKRINCH_TEST_SESSION_GROUP`.
fn session_group() -> Pid {
    let group = std::env::var("DESKRINCH_TEST_SESSION_GROUP")
        .expect("cargo xtask compositor-tests names the session to end");
    Pid::from_raw(group.parse().expect("a process group id")).expect("a positive id")
}

#[test]
#[ignore = "ends the nested compositor: cargo xtask compositor-tests runs it last"]
fn a_lost_connection_ends_the_stream_and_empties_the_list() {
    Compositor::current();
    let group = session_group();
    let app_id = unique_app_id("doomed-session");
    let watcher = Watcher::connect();
    let client = TestClient::connect();
    let _window = client.open("Doomed", &app_id);
    let id = watcher.wait_window("the window", &app_id, |_| true).id;

    // As if the compositor crashed: no goodbye, the socket just closes.
    kill_process_group(group, Signal::KILL).expect("end the session");

    let reason = watcher.wait_ended("the end of the connection");
    assert!(matches!(reason, EndReason::ConnectionLost(_)), "{reason:?}");
    assert_eq!(watcher.manager.windows(), []);
    let error = watcher.manager.perform(id, Action::Activate).unwrap_err();
    assert!(matches!(error, ActionError::Ended), "{error:?}");
}

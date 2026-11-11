//! The compositor going away under a connected manager. This ends the
//! session, so `cargo xtask compositor-tests` runs it last and alone, and
//! tells it the session's process group.
//!
//! Signalling a process group is only safe when it is the right one: a wrong
//! value would kill an unrelated group of the user's, and group 1 (or 0, or
//! a negative id) would reach every process the user owns. So the test
//! proves the target first: the group must be the one the compositor at the
//! other end of its own Wayland connection runs in, and neither init's nor
//! the test's own.

use deskrinch_window_management::{Action, ActionError, EndReason, WindowManagement};
use rustix::net::sockopt::socket_peercred;
use rustix::process::{Pid, RawPid, Signal, getpgid, getpgrp, kill_process_group};
use wayland_client::Connection;

use crate::support::{Compositor, Watcher, unique_app_id};
use deskrinch_test_client::TestClient;

/// The variable `cargo xtask compositor-tests` names the session's group in.
const GROUP_VARIABLE: &str = "DESKRINCH_TEST_SESSION_GROUP";

/// The process group named in the environment, if it is safe to end: it is
/// above 1, is not `own` (the test's group), and is `compositor`'s (the group
/// of the process serving the test's Wayland connection).
fn checked_group(named: &str, compositor: RawPid, own: RawPid) -> Result<Pid, String> {
    let group: RawPid = named
        .trim()
        .parse()
        .map_err(|_| format!("{GROUP_VARIABLE}={named:?} is not a process group id"))?;
    if group <= 1 {
        return Err(format!(
            "{GROUP_VARIABLE}={group} would signal far more than the session"
        ));
    }
    if group == own {
        return Err(format!("{GROUP_VARIABLE}={group} is the test's own group"));
    }
    if group != compositor {
        return Err(format!(
            "{GROUP_VARIABLE}={group} is not the compositor's group ({compositor})"
        ));
    }
    Pid::from_raw(group).ok_or_else(|| format!("{GROUP_VARIABLE}={group} is not positive"))
}

/// The group of the process at the other end of a Wayland connection to the
/// session's compositor, from the socket's peer credentials.
fn compositor_group() -> RawPid {
    let connection = Connection::connect_to_env().expect("connect to the compositor");
    let peer = socket_peercred(connection.backend().poll_fd()).expect("the compositor's pid");
    getpgid(Some(peer.pid))
        .expect("the compositor's process group")
        .as_raw_nonzero()
        .get()
}

/// The session's process group, proved to be the compositor's.
fn session_group() -> Pid {
    let named = std::env::var(GROUP_VARIABLE)
        .unwrap_or_else(|_| panic!("cargo xtask compositor-tests sets {GROUP_VARIABLE}"));
    let own = getpgrp().as_raw_nonzero().get();
    checked_group(&named, compositor_group(), own)
        .unwrap_or_else(|refusal| panic!("refusing to end the session: {refusal}"))
}

#[test]
#[ignore = "ends the nested compositor: cargo xtask compositor-tests runs it last"]
fn a_lost_connection_ends_the_stream_and_empties_the_list() {
    Compositor::current();
    let group = session_group();
    let app_id = unique_app_id("doomed-session");
    let watcher = Watcher::connect();
    let client = TestClient::connect().unwrap();
    let _window = client.open("Doomed", &app_id).unwrap();
    let id = watcher.wait_window("the window", &app_id, |_| true).id;

    // As if the compositor crashed: no goodbye, the socket just closes.
    kill_process_group(group, Signal::KILL).expect("end the session");

    let reason = watcher.wait_ended("the end of the connection");
    assert!(matches!(reason, EndReason::ConnectionLost(_)), "{reason:?}");
    assert_eq!(watcher.manager.windows(), []);
    let error = watcher.manager.perform(id, Action::Activate).unwrap_err();
    assert!(matches!(error, ActionError::Ended), "{error:?}");
}

#[test]
fn the_compositors_group_is_accepted() {
    assert_eq!(
        checked_group("4242", 4242, 100).map(Pid::as_raw_nonzero),
        Ok(Pid::from_raw(4242).unwrap().as_raw_nonzero())
    );
    assert!(checked_group(" 4242\n", 4242, 100).is_ok());
}

#[test]
fn groups_that_would_reach_beyond_the_session_are_refused() {
    for named in ["1", "0", "-1", "-4242"] {
        let refusal = checked_group(named, 4242, 100).unwrap_err();
        assert!(
            refusal.contains("far more than the session"),
            "{named}: {refusal}"
        );
    }
}

#[test]
fn the_tests_own_group_is_refused() {
    // Even when it serves the connection: then the test runs inside the
    // compositor's group, and would kill itself and its runner.
    let refusal = checked_group("100", 100, 100).unwrap_err();
    assert!(refusal.contains("the test's own group"), "{refusal}");
}

#[test]
fn a_group_that_is_not_the_compositors_is_refused() {
    let refusal = checked_group("4243", 4242, 100).unwrap_err();
    assert!(refusal.contains("not the compositor's group"), "{refusal}");
}

#[test]
fn a_value_that_is_not_a_number_is_refused() {
    for named in ["", "abc", "42 43", "99999999999999"] {
        let refusal = checked_group(named, 4242, 100).unwrap_err();
        assert!(
            refusal.contains("is not a process group id"),
            "{named}: {refusal}"
        );
    }
}

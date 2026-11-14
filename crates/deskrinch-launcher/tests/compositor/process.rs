//! The process's behaviour, through what it can be seen doing without
//! looking at the screen: who owns the launcher's name on the bus, what the
//! bus and `deskrinch-launcher toggle|show|hide` answer, how the processes
//! exit, and the frames the launcher presents (rinch's `RINCH_PERF` lines:
//! a hidden launcher presents none, a shown one does).

use std::process::ExitStatus;

use deskrinch_bus::Bus;
use deskrinch_launcher::{Answer, Request, send};
use rustix::process::{Pid, Signal, kill_process};

use crate::support::{Launcher, client, in_nested_session, launcher, owner_pid, run, stays};
use deskrinch_test_wait::{PATIENCE, comes_true};

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn starts_hidden_and_draws_nothing() {
    in_nested_session();
    let launcher = Launcher::start(&[]);
    assert!(stays(|| launcher.frames() == 0), "a hidden launcher drew");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn toggle_shows_it_and_toggles_it_back() {
    in_nested_session();
    let launcher = Launcher::start(&[]);
    assert!(client(&["toggle"]).success());
    assert!(
        comes_true(|| launcher.frames() > 0),
        "a toggle did not show it"
    );
    // The next toggle hides it: the bus's own toggle then shows it again.
    assert!(client(&["toggle"]).success());
    assert_eq!(
        send(&Bus::Session, Request::Toggle).unwrap(),
        Answer::Done { shown: Some(true) }
    );
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn show_and_hide_set_it() {
    in_nested_session();
    let launcher = Launcher::start(&[]);
    assert!(client(&["show"]).success());
    assert!(comes_true(|| launcher.frames() > 0));
    assert!(client(&["show"]).success());
    // Shown twice is shown: a toggle now hides it.
    assert_eq!(
        send(&Bus::Session, Request::Toggle).unwrap(),
        Answer::Done { shown: Some(false) }
    );
    assert!(client(&["show"]).success());
    assert!(client(&["hide"]).success());
    assert_eq!(
        send(&Bus::Session, Request::Toggle).unwrap(),
        Answer::Done { shown: Some(true) }
    );
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_second_launcher_leaves_the_first_running() {
    in_nested_session();
    let launcher = Launcher::start(&[]);
    let first = launcher.id();
    let second = launcher_exit(&[]);
    assert!(second.success(), "{second:?}");
    assert_eq!(owner_pid(), Some(first));
    assert!(stays(|| launcher.frames() == 0), "the first was shown");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_second_launcher_asked_to_show_shows_the_first() {
    in_nested_session();
    let launcher = Launcher::start(&[]);
    let first = launcher.id();
    let second = launcher_exit(&["--show"]);
    assert!(second.success(), "{second:?}");
    assert_eq!(owner_pid(), Some(first));
    assert!(
        comes_true(|| launcher.frames() > 0),
        "the first was not shown"
    );
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn with_none_running_hide_does_nothing() {
    in_nested_session();
    assert_eq!(owner_pid(), None);
    assert!(client(&["hide"]).success());
    assert!(stays(|| owner_pid().is_none()), "a hide started a launcher");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn with_none_running_toggle_starts_one_shown() {
    in_nested_session();
    assert_eq!(owner_pid(), None);
    // The launcher the client starts inherits its output.
    let mut toggle = run(launcher(&["toggle"]));
    let status = toggle.wait_exit(PATIENCE).unwrap();
    assert!(status.is_some_and(|status| status.success()));
    assert!(comes_true(|| owner_pid().is_some()), "no launcher started");
    let started = owner_pid().unwrap();
    let shown = comes_true(|| toggle.watch().frames() > 0);
    // It runs in a process group of its own, so the session would not
    // end it: this test does, before it asserts anything more. (The binary
    // under test starts it, so it cannot be bound to this process; should
    // this test be killed before here, it ends when the nested session's
    // compositor does, its display gone.)
    let pid = Pid::from_raw(i32::try_from(started).unwrap()).unwrap();
    kill_process(pid, Signal::TERM).unwrap();
    assert!(comes_true(|| owner_pid().is_none()), "it did not end");
    assert!(shown, "the started launcher was not shown");
}

/// Runs a second `deskrinch-launcher` with `args`, which must leave
/// within [`PATIENCE`].
fn launcher_exit(args: &[&str]) -> ExitStatus {
    let mut second = run(launcher(args));
    // Dropped, it is killed: whether it left or not.
    second
        .wait_exit(PATIENCE)
        .unwrap()
        .expect("a second launcher did not leave")
}

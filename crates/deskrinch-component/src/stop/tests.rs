//! Each case runs in a child process, this test binary run again with only
//! [`child`] (ignored, so a plain run passes over it) and the case named in
//! [`CASE`]: a signal ends only the child. The parent reads how the child
//! ended, when, and what it said.
//!
//! Timings are judged by the bound's own clock, not the scheduler's: an
//! end after the bound can only be later on a loaded machine, never
//! earlier, so "not before the bound" is sound; an end before it is shown
//! by what the child did not say (the bound's line), not by milliseconds.

use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use deskrinch_test_wait::PATIENCE;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::low_level::raise;

use super::{BOUND, asked_line, bound_line, install_once};

/// The variable naming the case the child runs.
const CASE: &str = "DESKRINCH_STOP_CASE";

/// The bound of the stuck case: short, so the test is.
const SHORT_BOUND: Duration = Duration::from_millis(300);

/// How the child ended, how long after it was started, and what it said.
struct Ended {
    status: ExitStatus,
    after: Duration,
    said: String,
}

/// Runs the child with `case`, through `sh -c 'PREFIX exec "$0" "$@"'`
/// when `prefix` is given (a `trap` the child inherits).
fn run_child_with(case: &str, prefix: Option<&str>) -> Ended {
    let exe = std::env::current_exe().unwrap();
    let test_args = [
        "--exact",
        "stop::tests::child",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ];
    let mut command = match prefix {
        Some(prefix) => {
            let mut command = Command::new("/bin/sh");
            command
                .arg("-c")
                .arg(format!("{prefix} exec \"$0\" \"$@\""))
                .arg(&exe);
            command
        }
        None => Command::new(&exe),
    };
    command
        .args(test_args)
        .env(CASE, case)
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let output = command.output().unwrap();
    Ended {
        status: output.status,
        after: started.elapsed(),
        said: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn run_child(case: &str) -> Ended {
    run_child_with(case, None)
}

#[track_caller]
fn ended_in_order(ended: &Ended, signal: i32) {
    assert_eq!(
        ended.status.code(),
        Some(0),
        "it ended {}; it said:\n{}",
        ended.status,
        ended.said
    );
    assert!(
        ended.said.contains(&asked_line(signal)),
        "it did not say {:?}; it said:\n{}",
        asked_line(signal),
        ended.said
    );
    assert!(!ended.said.contains("still running"), "{}", ended.said);
}

#[test]
fn sigterm_asks_it_to_end_and_it_ends_with_success() {
    ended_in_order(&run_child("ends:TERM"), SIGTERM);
}

#[test]
fn sigint_does_too() {
    ended_in_order(&run_child("ends:INT"), SIGINT);
}

#[test]
fn sighup_does_too() {
    ended_in_order(&run_child("ends:HUP"), SIGHUP);
}

#[test]
fn a_loop_that_does_not_end_is_ended_by_the_signal_after_the_bound() {
    let ended = run_child("stuck");
    assert_eq!(
        ended.status.signal(),
        Some(SIGTERM),
        "it ended {}; it said:\n{}",
        ended.status,
        ended.said
    );
    assert!(
        ended.after >= SHORT_BOUND,
        "it ended {:?} after it started, before the bound",
        ended.after
    );
    assert!(
        ended.after < SHORT_BOUND + PATIENCE,
        "it ended only {:?} after it started",
        ended.after
    );
    assert!(ended.said.contains(&asked_line(SIGTERM)), "{}", ended.said);
    let line = bound_line(SIGTERM, SHORT_BOUND);
    assert!(ended.said.contains(&line), "{}", ended.said);
    assert!(
        ended
            .said
            .lines()
            .any(|said| said.contains(&line) && said.contains(" WARN ")),
        "the bound's line is not a warning: {}",
        ended.said
    );
}

#[test]
fn a_second_signal_ends_it_at_once_by_the_signal() {
    let ended = run_child("twice");
    assert_eq!(
        ended.status.signal(),
        Some(SIGTERM),
        "it ended {}; it said:\n{}",
        ended.status,
        ended.said
    );
    assert!(ended.said.contains(&asked_line(SIGTERM)), "{}", ended.said);
    // The bound (the patience, in this case) never ran out: it ended by
    // the second signal, not by the bound.
    assert!(!ended.said.contains("still running"), "{}", ended.said);
    assert!(ended.after < PATIENCE, "{:?}", ended.after);
}

#[test]
fn a_signal_ignored_when_it_started_stays_ignored() {
    let ended = run_child_with("ignored-hup", Some("trap '' HUP;"));
    ended_in_order(&ended, SIGTERM);
    assert!(!ended.said.contains(&asked_line(SIGHUP)), "{}", ended.said);
}

#[test]
fn installing_twice_installs_once() {
    let ended = run_child("installed-twice");
    ended_in_order(&ended, SIGTERM);
    assert_eq!(
        ended.said.matches(&asked_line(SIGTERM)).count(),
        1,
        "{}",
        ended.said
    );
}

#[test]
fn the_component_bound_is_five_seconds() {
    assert_eq!(BOUND, Duration::from_secs(5));
}

/// Set by the ask, waited for by the child's main.
#[derive(Default)]
struct Asked {
    times: AtomicUsize,
    done: Mutex<bool>,
    changed: Condvar,
}

impl Asked {
    fn ask(&self) {
        self.times.fetch_add(1, Ordering::SeqCst);
        *self.done.lock().unwrap() = true;
        self.changed.notify_all();
    }

    /// Waits, within the patience, for the ask; whether it came.
    fn wait(&self) -> bool {
        let done = self.done.lock().unwrap();
        let (done, _) = self
            .changed
            .wait_timeout_while(done, PATIENCE, |done| !*done)
            .unwrap();
        *done
    }

    fn came(&self) -> bool {
        *self.done.lock().unwrap()
    }
}

/// An ask that ends the child's main in order, and the state it sets.
fn asking() -> (Arc<Asked>, impl Fn() + Send + 'static) {
    let asked = Arc::new(Asked::default());
    let handle = Arc::clone(&asked);
    (asked, move || handle.ask())
}

fn never_returns() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// The child: runs the case [`CASE`] names, or nothing without one.
#[test]
#[ignore = "run by this module's tests, in a child process"]
fn child() {
    let Ok(case) = std::env::var(CASE) else {
        return;
    };
    crate::report::init();
    match case.as_str() {
        "ends:TERM" | "ends:INT" | "ends:HUP" => {
            let signal = match case.as_str() {
                "ends:TERM" => SIGTERM,
                "ends:INT" => SIGINT,
                _ => SIGHUP,
            };
            let (asked, ask) = asking();
            install_once(ask, BOUND).unwrap();
            raise(signal).unwrap();
            assert!(asked.wait(), "the ask did not come");
        }
        "stuck" => {
            install_once(|| {}, SHORT_BOUND).unwrap();
            raise(SIGTERM).unwrap();
            never_returns();
        }
        "twice" => {
            // A bound that will not run out: only the second signal can end it.
            install_once(|| {}, PATIENCE).unwrap();
            raise(SIGTERM).unwrap();
            std::thread::sleep(Duration::from_millis(200));
            raise(SIGTERM).unwrap();
            never_returns();
        }
        "ignored-hup" => {
            let (asked, ask) = asking();
            install_once(ask, BOUND).unwrap();
            raise(SIGHUP).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            assert!(!asked.came(), "SIGHUP was acted on");
            raise(SIGTERM).unwrap();
            assert!(asked.wait(), "the ask did not come");
        }
        "installed-twice" => {
            let (asked, ask) = asking();
            install_once(ask, BOUND).unwrap();
            let (_, other) = asking();
            install_once(other, BOUND).unwrap();
            raise(SIGTERM).unwrap();
            assert!(asked.wait(), "the ask did not come");
            // Time for a second thread, had one been started, to say so.
            std::thread::sleep(Duration::from_millis(300));
            assert_eq!(asked.times.load(Ordering::SeqCst), 1);
        }
        other => panic!("no case {other:?}"),
    }
}

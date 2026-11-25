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

use deskrinch_shell::exit_status::stops_in_order_line;

use super::{BOUND, asked_line, bound_line, ignored, install_once, name};

/// The variable naming the case the child runs.
const CASE: &str = "DESKRINCH_STOP_CASE";

/// The bound of the stuck case: short, so the test is.
const SHORT_BOUND: Duration = Duration::from_millis(300);

/// How the child ended, how long after it was started, what it said, and
/// its process id.
struct Ended {
    status: ExitStatus,
    after: Duration,
    said: String,
    pid: u32,
}

/// The start of the message a case gives, at once, when the signal it
/// relies on is ignored in the test process.
const IGNORED_HERE: &str = "is ignored in this test process";

/// Fails at once, saying why, when `signal` is ignored (`SIG_IGN`) in this
/// test process. A child inherits that, and keeps it ignored by design
/// (the module's doc), so a case relying on it would wait out the patience
/// and fail as if the stop were broken. `nohup` starts a process with
/// `SIGHUP` ignored; a shell without job control starts a background job
/// with `SIGINT` ignored.
#[track_caller]
fn not_ignored_here(signal: i32) {
    assert!(
        !ignored(signal),
        "{} {IGNORED_HERE} (started under nohup, or as a background job?): \
         the child would inherit that and keep it ignored, so this case \
         cannot show the stop here; run the tests with it not ignored",
        name(signal)
    );
}

/// Runs the child with `case`, through `sh -c 'PREFIX exec "$0" "$@"'`
/// when `prefix` is given (a `trap` the child inherits). The case relies
/// on `relies_on` reaching the child: the run fails at once if it is
/// ignored here ([`not_ignored_here`]).
#[track_caller]
fn run_child_with(case: &str, prefix: Option<&str>, relies_on: i32) -> Ended {
    not_ignored_here(relies_on);
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
        .env("RUST_LOG", "debug")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let child = command.spawn().unwrap();
    // With a prefix, `exec` keeps the shell's process: the id is the child's.
    let pid = child.id();
    let output = child.wait_with_output().unwrap();
    Ended {
        status: output.status,
        after: started.elapsed(),
        said: String::from_utf8_lossy(&output.stderr).into_owned(),
        pid,
    }
}

#[track_caller]
fn run_child(case: &str, relies_on: i32) -> Ended {
    run_child_with(case, None, relies_on)
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
    let installed = stops_in_order_line(ended.pid);
    assert!(
        ended.said.contains(&installed),
        "it did not say {installed:?}; it said:\n{}",
        ended.said
    );
}

#[test]
fn sigterm_asks_it_to_end_and_it_ends_with_success() {
    ended_in_order(&run_child("ends:TERM", SIGTERM), SIGTERM);
}

#[test]
fn sigint_does_too() {
    ended_in_order(&run_child("ends:INT", SIGINT), SIGINT);
}

#[test]
fn sighup_does_too() {
    ended_in_order(&run_child("ends:HUP", SIGHUP), SIGHUP);
}

#[test]
fn a_loop_that_does_not_end_is_ended_by_the_signal_after_the_bound() {
    let ended = run_child("stuck", SIGTERM);
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
    let ended = run_child("twice", SIGTERM);
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
    let ended = run_child_with("ignored-hup", Some("trap '' HUP;"), SIGTERM);
    ended_in_order(&ended, SIGTERM);
    assert!(!ended.said.contains(&asked_line(SIGHUP)), "{}", ended.said);
}

#[test]
fn a_case_whose_signal_is_ignored_here_says_so_at_once() {
    // This test binary again, running only `sighup_does_too`, with SIGHUP
    // ignored as `nohup` leaves it.
    let exe = std::env::current_exe().unwrap();
    let started = Instant::now();
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg("trap '' HUP; exec \"$0\" \"$@\"")
        .arg(&exe)
        .args([
            "--exact",
            "stop::tests::sighup_does_too",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let after = started.elapsed();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "it passed; it said:\n{said}");
    let message = format!("SIGHUP {IGNORED_HERE}");
    assert!(
        said.contains(&message),
        "it did not say {message:?}; it said:\n{said}"
    );
    // Without the check it fails too, but only once the child's patience
    // has run out.
    assert!(after < PATIENCE, "it took {after:?}; it said:\n{said}");
}

#[test]
fn installing_twice_installs_once() {
    let ended = run_child("installed-twice", SIGTERM);
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

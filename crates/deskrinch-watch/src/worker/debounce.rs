//! The debounce's contract, with made-up times: a backend whose every wait
//! the test answers ([`Scripted`]) and a clock the test moves ([`Fake`]).
//!
//! Each wait the worker makes is announced to the test, with the time it
//! may wait, on the same channel as the reports it makes; the test then
//! moves the clock and answers. So what the test reads is the exact
//! sequence of what the worker did, and "one report" is shown by what
//! comes after it, not by a stretch of real time with nothing in it.

use super::*;
use crate::clock::Clock;
use deskrinch_test_wait::recv;
use std::sync::Mutex;
use std::time::Instant;

/// What the worker did next, as the test reads it.
#[derive(Debug)]
enum Next {
    /// It waits, for at most this long (none: until something comes).
    Waits(Option<Duration>),
    /// It reported this.
    Reported(Event),
}

/// A clock the test moves.
#[derive(Clone)]
struct Fake(Arc<Mutex<Instant>>);

impl Clock for Fake {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }
}

/// A backend whose waits the test answers: each wait says so, with its
/// time, then takes the next scripted answer. Its waker answers too.
struct Scripted {
    next: Sender<Next>,
    answers: Receiver<Wait>,
    wake: Sender<Wait>,
}

impl Backend for Scripted {
    fn watch(&mut self, _dir: &Path, _how: How) -> Result<(), WatchError> {
        Ok(())
    }

    fn unwatch(&mut self, _dir: &Path) {}

    fn wait(&mut self, timeout: Option<Duration>) -> Wait {
        self.next.send(Next::Waits(timeout)).ok();
        self.answers
            .recv()
            .unwrap_or_else(|_| Wait::Failed(WatchError::from_errno(rustix::io::Errno::BADF, None)))
    }

    fn waker(&self) -> Waker {
        let wake = Mutex::new(self.wake.clone());
        Arc::new(move || {
            wake.lock().unwrap().send(Wait::Woken).ok();
        })
    }
}

/// A started watcher on a [`Scripted`] backend and a [`Fake`] clock, with
/// the default timing.
struct Run {
    next: Receiver<Next>,
    answers: Sender<Wait>,
    clock: Fake,
    timing: Timing,
    watcher: Watcher,
    _root: tempfile::TempDir,
}

const MS: Duration = Duration::from_millis(1);

/// `a` less `b`, which a test's arithmetic keeps below `a`.
fn less(a: Duration, b: Duration) -> Duration {
    a.checked_sub(b).expect("a duration less a longer one")
}

impl Run {
    fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        let (next, read) = mpsc::channel();
        let (answers, scripted) = mpsc::channel();
        let clock = Fake(Arc::new(Mutex::new(Instant::now())));
        let backend = Scripted {
            next: next.clone(),
            answers: scripted,
            wake: answers.clone(),
        };
        let timing = Timing::default();
        let watcher = Armed::with_parts(
            Box::new(backend),
            Box::new(clock.clone()),
            vec![Target::Tree(root.path().to_path_buf())],
            timing,
        )
        .unwrap()
        .start("test-debounce", move |event| {
            next.send(Next::Reported(event)).ok();
        })
        .unwrap();
        Self {
            next: read,
            answers,
            clock,
            timing,
            watcher,
            _root: root,
        }
    }

    /// The worker's next wait, which must be one for at most `budget`.
    fn waits(&self, budget: Option<Duration>) {
        match recv("what the worker did next", &self.next) {
            Next::Waits(asked) => assert_eq!(asked, budget, "the worker waits, for the wrong time"),
            Next::Reported(event) => {
                panic!("the worker was to wait for {budget:?}, but reported {event:?}")
            }
        }
    }

    /// The worker's next act, which must be a report of a change.
    fn reports(&self) {
        match recv("what the worker did next", &self.next) {
            Next::Reported(Event::Changed) => {}
            Next::Reported(event) => panic!("the worker was to report a change, not {event:?}"),
            Next::Waits(asked) => {
                panic!("the worker was to report a change, but waits for {asked:?}")
            }
        }
    }

    fn advance(&self, by: Duration) {
        *self.clock.0.lock().unwrap() += by;
    }

    /// Answers the wait with a change that concerns a target.
    fn change(&self) {
        self.answers
            .send(Wait::Changes(vec![Change::Unknown]))
            .unwrap();
    }

    /// Answers the wait with its time run out.
    fn quiet(&self) {
        self.answers.send(Wait::Quiet).unwrap();
    }
}

#[test]
fn a_burst_with_no_pause_as_long_as_quiet_is_one_report() {
    let run = Run::start();
    let quiet = run.timing.quiet;
    run.waits(None);
    run.change();
    for _ in 0..3 {
        run.waits(Some(quiet));
        run.advance(less(quiet, MS));
        run.change();
    }
    run.waits(Some(quiet));
    run.advance(quiet);
    run.quiet();
    run.reports();
    // The next thing it does is wait for a change: the burst was one report.
    run.waits(None);
    drop(run.watcher);
}

#[test]
fn a_pause_of_quiet_separates_two_reports() {
    let run = Run::start();
    let quiet = run.timing.quiet;
    for _ in 0..2 {
        run.waits(None);
        run.change();
        run.waits(Some(quiet));
        run.advance(quiet);
        run.quiet();
        run.reports();
    }
    run.waits(None);
}

#[test]
fn a_stream_that_never_pauses_is_reported_max_delay_after_its_first_change() {
    let run = Run::start();
    let (quiet, max_delay) = (run.timing.quiet, run.timing.max_delay);
    let step = less(quiet, 10 * MS);
    run.waits(None);
    run.change();
    // Each wait may last a quiet pause, or what is left of `max_delay`
    // since the first change, whichever is shorter.
    let mut elapsed = Duration::ZERO;
    loop {
        let left = less(max_delay, elapsed);
        run.waits(Some(quiet.min(left)));
        if left <= step {
            // The last wait before the report: what is left of `max_delay`,
            // not a quiet pause. The stream goes on, but its time is up.
            assert!(left < quiet, "{left:?}");
            run.advance(left);
            run.quiet();
            break;
        }
        run.advance(step);
        elapsed += step;
        run.change();
    }
    run.reports();
    // The stream goes on: its next change starts the next report's burst,
    // gathered the same way.
    run.waits(None);
    run.change();
    run.waits(Some(quiet));
    run.advance(step);
    run.change();
    run.waits(Some(quiet));
    run.advance(quiet);
    run.quiet();
    run.reports();
    run.waits(None);
}

#[test]
fn a_time_not_yet_run_out_is_waited_out() {
    // A wait that ends before its time (a change the plan does not watch
    // for, a spurious wake) is followed by one for what is left.
    let run = Run::start();
    let quiet = run.timing.quiet;
    run.waits(None);
    run.change();
    run.waits(Some(quiet));
    run.advance(40 * MS);
    run.quiet();
    run.waits(Some(less(quiet, 40 * MS)));
    run.advance(less(quiet, 40 * MS));
    run.quiet();
    run.reports();
}

#[test]
fn a_refresh_when_nothing_is_changing_reports_at_once() {
    let run = Run::start();
    run.waits(None);
    run.watcher.refresh();
    run.reports();
    run.waits(None);
    // New targets the same way.
    run.watcher
        .handle()
        .set_targets(vec![Target::File(PathBuf::from("/nowhere/f"))]);
    run.reports();
    run.waits(None);
}

#[test]
fn a_refresh_during_a_burst_joins_its_one_report() {
    let run = Run::start();
    let quiet = run.timing.quiet;
    run.waits(None);
    run.change();
    run.waits(Some(quiet));
    run.advance(30 * MS);
    run.watcher.refresh();
    // It is heard, and the burst goes on: the worker waits out the rest of
    // its pause, then reports once.
    run.waits(Some(quiet));
    run.advance(quiet);
    run.quiet();
    run.reports();
    run.waits(None);
}

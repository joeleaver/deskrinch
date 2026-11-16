//! The worker's waits and turns with the zone changed as `timedatectl`
//! changes it: a new link made beside `localtime`, then renamed over it, two
//! events in the watched directory. The worker may wake between them, and
//! that wake has nothing set.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use jiff::SignedDuration;
use tempfile::TempDir;

use super::*;
use crate::test_zones::SYSTEM_ZONEINFO;
use deskrinch_test_wait::PATIENCE;

/// A worker following `localtime`, a link to Berlin's zone in a directory
/// of the test's own, with what it tells its subscriber.
struct Following {
    dir: TempDir,
    localtime: PathBuf,
    worker: Worker,
    events: Receiver<Event>,
}

impl Following {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        symlink(format!("{SYSTEM_ZONEINFO}/Europe/Berlin"), &localtime).unwrap();
        let (send, events) = mpsc::channel();
        let worker = Worker {
            env: Environment {
                tz: None,
                zoneinfo: PathBuf::from(SYSTEM_ZONEINFO),
                localtime: localtime.clone(),
                time_locale: None,
            },
            timer: WallTimer::new().unwrap(),
            watch: ZoneWatch::new().unwrap(),
            stop: Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).unwrap()),
            subscriber: Box::new(move |event| send.send(event).unwrap()),
        };
        Self {
            dir,
            localtime,
            worker,
            events,
        }
    }

    /// timedatectl's first step: a new link to Tokyo's zone beside
    /// `localtime`. Returns it.
    fn make_new_link(&self) -> PathBuf {
        let new_link = self.dir.path().join(".localtime.new");
        symlink(format!("{SYSTEM_ZONEINFO}/Asia/Tokyo"), &new_link).unwrap();
        new_link
    }

    /// Watches `localtime` and arms the timer an hour ahead, so that an
    /// armed wait can end only for the watch (or the stop signal).
    fn watch_with_the_timer_an_hour_ahead(&mut self) {
        self.worker.watch.watch(Some(&self.localtime)).unwrap();
        let in_an_hour = Timestamp::now() + SignedDuration::from_hours(1);
        assert_eq!(self.worker.timer.arm(in_an_hour).unwrap(), Fired::Pending);
    }

    /// The ticks told so far, by cause; an error told fails the test.
    fn causes_told(&self) -> Vec<Cause> {
        self.events
            .try_iter()
            .map(|event| match event {
                Event::Tick(tick) => tick.cause,
                Event::Error(error) => panic!("the clock told an error: {error}"),
            })
            .collect()
    }
}

/// The worker waiting on a thread of its own, wait after wait, each wake
/// sent to the receiver, until one for the zone.
fn wait_until_the_zone(mut worker: Worker) -> (Receiver<Woke>, JoinHandle<()>) {
    let (send, wakes) = mpsc::channel();
    let waiting = thread::spawn(move || {
        loop {
            let woke = worker.wait(true);
            let done = woke.zone || woke.stop;
            send.send(woke).unwrap();
            if done {
                return;
            }
        }
    });
    (wakes, waiting)
}

fn next_wake(wakes: &Receiver<Woke>) -> Woke {
    wakes
        .recv_timeout(PATIENCE)
        .expect("the worker woke, an hour before its timer")
}

fn nothing_set(woke: &Woke) -> bool {
    !woke.stop && !woke.zone && !woke.clock_set
}

/// The worker does not look for a change of zone from time to time: it
/// blocks on the zone watch, with the timer and its stop signal, and wakes
/// because the watch is ready. Shown without a bound on time: the timer is
/// armed an hour ahead and an armed wait has no timeout, so the wait can
/// only return early for the watch. The two events may come in one wake or
/// in two; a wake before the rename has nothing set.
#[test]
fn a_zone_change_wakes_a_waiting_worker_by_itself() {
    let mut following = Following::new();
    following.watch_with_the_timer_an_hour_ahead();
    let new_link = following.make_new_link();
    let (wakes, waiting) = wait_until_the_zone(following.worker);
    fs::rename(&new_link, &following.localtime).unwrap();

    let mut wake = next_wake(&wakes);
    while nothing_set(&wake) {
        wake = next_wake(&wakes);
    }
    assert!(wake.zone, "{wake:?}");
    assert!(!wake.stop && !wake.clock_set, "{wake:?}");
    waiting.join().unwrap();
}

/// The two events forced apart: the worker's wake for the new link alone
/// has nothing set, and the rename then wakes it for the zone.
#[test]
fn a_wake_for_the_new_link_alone_has_nothing_set() {
    let mut following = Following::new();
    following.watch_with_the_timer_an_hour_ahead();
    // The new link's event is the only one queued for the first wait.
    let new_link = following.make_new_link();
    let (wakes, waiting) = wait_until_the_zone(following.worker);
    let first = next_wake(&wakes);
    assert!(nothing_set(&first), "{first:?}");
    fs::rename(&new_link, &following.localtime).unwrap();

    let second = next_wake(&wakes);
    assert!(second.zone, "{second:?}");
    assert!(!second.stop && !second.clock_set, "{second:?}");
    waiting.join().unwrap();
}

/// What the panel relies on: with the two events forced apart, the
/// worker's turns call the subscriber once, for the zone, and not for the
/// wake between them. A minute passing in the meantime ticks for the
/// boundary, which is allowed; nothing else is.
#[test]
fn a_wake_with_nothing_set_reaches_no_subscriber() {
    let mut following = Following::new();
    let mut ticker = following.worker.begin(Precision::Minutes);
    assert_eq!(following.causes_told(), [Cause::Start]);

    let new_link = following.make_new_link();
    // Turns until the new link's event is read: one, or more if a minute's
    // boundary woke a turn first.
    loop {
        assert_eq!(following.worker.turn(&mut ticker, false), Some(false));
        let mut watch = [PollFd::new(&following.worker.watch, PollFlags::IN)];
        let zero = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if poll(&mut watch, Some(&zero)).unwrap() == 0 {
            break;
        }
    }
    let before_the_rename = following.causes_told();
    assert!(
        before_the_rename
            .iter()
            .all(|cause| *cause == Cause::Boundary),
        "{before_the_rename:?}"
    );

    fs::rename(&new_link, &following.localtime).unwrap();
    let mut after_the_rename = Vec::new();
    while !after_the_rename.contains(&Cause::TimeZoneChanged) {
        assert_eq!(following.worker.turn(&mut ticker, false), Some(false));
        after_the_rename.extend(following.causes_told());
    }
    let zone_ticks = after_the_rename
        .iter()
        .filter(|cause| **cause != Cause::Boundary)
        .count();
    assert_eq!(zone_ticks, 1, "{after_the_rename:?}");
}

//! The clock's worker thread: waits on the wall-clock timer, the time zone
//! watch and a stop signal, and reports to the subscriber.
//!
//! Each turn arms the timer for the [`Ticker`]'s next boundary, then reads
//! the wall clock, then waits. Reading after arming matters: the timer is
//! cancelled only by a change of the clock after it was armed, so a change
//! just before is caught by that read instead.

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use rustix::event::{EventfdFlags, PollFd, PollFlags, eventfd, poll};
use rustix::io::Errno;
use rustix::time::Timespec;

use crate::boundary::Precision;
use crate::env::Environment;
use crate::error::ClockError;
use crate::ticker::{Cause, Tick, Ticker};
use crate::timer::{Fired, WallTimer};
use crate::watch::ZoneWatch;
use crate::zone;

/// What the clock tells its subscriber.
#[derive(Debug)]
pub enum Event {
    /// The displayed time changed, or the time zone did.
    Tick(Tick),
    /// Something went wrong; the clock carries on as well as it can.
    Error(ClockError),
}

/// A wall clock that calls a subscriber whenever the time it shows changes,
/// until dropped.
///
/// See the [crate documentation](crate) for when it wakes and why.
#[derive(Debug)]
pub struct Clock {
    stop: Arc<OwnedFd>,
    worker: Option<JoinHandle<()>>,
}

impl Clock {
    /// Starts a clock showing time to `precision` in the time zone `env`
    /// names. `subscriber` is called on the clock's own thread, one event
    /// at a time: first with a [`Tick`] of [`Cause::Start`] (preceded by
    /// an [`Event::Error`] if the time zone is unusable), then at every
    /// change.
    ///
    /// # Errors
    ///
    /// [`ClockError`] when the timer, the inotify instance or the thread
    /// cannot be created. Problems after that, such as an unreadable time
    /// zone, are reported to the subscriber as [`Event::Error`].
    pub fn start(
        env: Environment,
        precision: Precision,
        subscriber: impl Fn(Event) + Send + 'static,
    ) -> Result<Self, ClockError> {
        let timer = WallTimer::new().map_err(ClockError::Timer)?;
        let watch = ZoneWatch::new().map_err(ClockError::Watch)?;
        let stop = Arc::new(
            eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
                .map_err(|error| ClockError::Thread(error.into()))?,
        );
        let worker = Worker {
            env,
            timer,
            watch,
            stop: Arc::clone(&stop),
            subscriber: Box::new(subscriber),
        };
        let handle = thread::Builder::new()
            .name("clock".into())
            .spawn(move || worker.run(precision))
            .map_err(ClockError::Thread)?;
        Ok(Self {
            stop,
            worker: Some(handle),
        })
    }
}

impl Drop for Clock {
    fn drop(&mut self) {
        // Writing to an eventfd fails only when its counter would overflow,
        // which one write cannot do.
        rustix::io::write(&*self.stop, &1_u64.to_ne_bytes()).ok();
        if let Some(worker) = self.worker.take() {
            // A panic in the subscriber ended the thread already.
            worker.join().ok();
        }
    }
}

/// How often the worker looks at the clock when the timer cannot be armed.
const UNARMED_POLL: Timespec = Timespec {
    tv_sec: 1,
    tv_nsec: 0,
};

struct Worker {
    env: Environment,
    timer: WallTimer,
    watch: ZoneWatch,
    stop: Arc<OwnedFd>,
    subscriber: Box<dyn Fn(Event) + Send>,
}

/// What woke the worker.
#[derive(Debug, Default)]
struct Woke {
    stop: bool,
    zone: bool,
    clock_set: bool,
}

impl Worker {
    fn run(mut self, precision: Precision) {
        let zone = self.load_zone();
        let (mut ticker, first) = Ticker::start(zone, precision, Timestamp::now());
        self.tell(Event::Tick(first));
        let mut clock_set = false;
        loop {
            let deadline = ticker.deadline();
            let armed = match self.timer.arm(deadline) {
                Ok(fired) => {
                    clock_set |= fired == Fired::ClockSet;
                    true
                }
                Err(error) => {
                    self.tell(Event::Error(ClockError::Timer(error)));
                    false
                }
            };
            let now = Timestamp::now();
            let cause = if clock_set || now < deadline {
                Cause::ClockChanged
            } else {
                Cause::Boundary
            };
            clock_set = false;
            if let Some(tick) = ticker.observe(now, cause) {
                self.tell(Event::Tick(tick));
            }
            if ticker.deadline() != deadline {
                continue;
            }
            let woke = self.wait(armed);
            if woke.stop {
                return;
            }
            if woke.zone {
                let zone = self.load_zone();
                if let Some(tick) = ticker.change_zone(zone, Timestamp::now()) {
                    self.tell(Event::Tick(tick));
                }
            }
            clock_set = woke.clock_set;
        }
    }

    fn tell(&self, event: Event) {
        (self.subscriber)(event);
    }

    /// Waits for the timer, the zone watch or the stop signal. When the
    /// timer is not armed, it looks at the clock every second instead: late
    /// by up to a second, the best that can be done without the timer.
    fn wait(&mut self, armed: bool) -> Woke {
        let timeout = (!armed).then_some(&UNARMED_POLL);
        let mut fds = [
            PollFd::new(&*self.stop, PollFlags::IN),
            PollFd::new(&self.watch, PollFlags::IN),
            PollFd::new(&self.timer, PollFlags::IN),
        ];
        let ready = match poll(&mut fds, timeout) {
            Ok(_) => fds.map(|fd| !fd.revents().is_empty()),
            Err(Errno::INTR) => [false; 3],
            Err(error) => {
                self.tell(Event::Error(ClockError::Timer(error.into())));
                return Woke::default();
            }
        };
        let mut woke = Woke {
            stop: ready[0],
            ..Woke::default()
        };
        if ready[1] {
            match self.watch.changed() {
                Ok(changed) => woke.zone = changed,
                Err(error) => {
                    // The events are lost; read the zone anyway, so the
                    // change they may have reported is not.
                    self.tell(Event::Error(ClockError::Watch(error)));
                    woke.zone = true;
                }
            }
        }
        if ready[2] {
            match self.timer.read() {
                Ok(fired) => woke.clock_set = fired == Fired::ClockSet,
                Err(error) => self.tell(Event::Error(ClockError::Timer(error))),
            }
        }
        woke
    }

    /// Reads the time zone, watching its file first so that a change while
    /// reading is noticed, and reports why it is unusable if it is.
    fn load_zone(&mut self) -> TimeZone {
        let mut watched = None;
        loop {
            let resolved = zone::resolve(&self.env);
            if resolved.file == watched {
                if let Some(error) = resolved.error {
                    self.tell(Event::Error(error.into()));
                }
                return resolved.zone;
            }
            if let Err(error) = self.watch.watch(resolved.file.as_deref()) {
                self.tell(Event::Error(ClockError::Watch(error)));
            }
            watched = resolved.file;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::sync::mpsc;

    use jiff::SignedDuration;

    use super::*;
    use deskrinch_test_wait::PATIENCE;

    /// The worker does not look for a change of zone from time to time: it
    /// blocks on the zone watch, with the timer and its stop signal, and
    /// wakes because the watch is ready. Shown without a bound on time:
    /// the timer is armed an hour ahead and an armed wait has no timeout,
    /// so the wait can only return early because the zone changed.
    #[test]
    fn a_zone_change_wakes_a_waiting_worker_by_itself() {
        let dir = tempfile::tempdir().unwrap();
        let (berlin, tokyo) = (dir.path().join("berlin"), dir.path().join("tokyo"));
        fs::write(&berlin, "b").unwrap();
        fs::write(&tokyo, "t").unwrap();
        let localtime = dir.path().join("localtime");
        symlink(&berlin, &localtime).unwrap();

        let mut watch = ZoneWatch::new().unwrap();
        watch.watch(Some(&localtime)).unwrap();
        let timer = WallTimer::new().unwrap();
        let in_an_hour = Timestamp::now() + SignedDuration::from_hours(1);
        assert_eq!(timer.arm(in_an_hour).unwrap(), Fired::Pending);
        let mut worker = Worker {
            env: Environment {
                tz: None,
                zoneinfo: PathBuf::from("/usr/share/zoneinfo"),
                localtime: localtime.clone(),
                time_locale: None,
            },
            timer,
            watch,
            stop: Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).unwrap()),
            subscriber: Box::new(|_| {}),
        };
        let (send, woke) = mpsc::channel();
        let waiting = thread::spawn(move || send.send(worker.wait(true)).unwrap());

        // timedatectl's way: a new link renamed over the old.
        let new_link = dir.path().join(".localtime.new");
        symlink(&tokyo, &new_link).unwrap();
        fs::rename(&new_link, &localtime).unwrap();

        let woke = woke
            .recv_timeout(PATIENCE)
            .expect("the worker woke for the zone, an hour before its timer");
        assert!(woke.zone, "{woke:?}");
        assert!(!woke.stop && !woke.clock_set, "{woke:?}");
        waiting.join().unwrap();
    }
}

//! The clock against the real wall clock and real time zone files.
//!
//! Setting the wall clock needs `CAP_SYS_TIME`, which a test does not have;
//! the clock's response to that is tested on the model (`ticker`) with a
//! clock the test controls. Here: the timer wakes at the boundary, the time
//! zone file is watched, and the clock stops when dropped.
//!
//! The tests claim what the clock promises, on the times its ticks carry,
//! not on when the test's thread happens to receive them: it never wakes
//! before a boundary; each tick carries the time it is; a wake the machine
//! made late shows the time it is (a later second when it is late by a
//! second or more), which the test says on its output and does not fail
//! on; a dropped clock's thread has ended when the drop returns. How late
//! the clock wakes in practice is a measurement (`cargo xtask measure`),
//! not an assertion. Every wait has `deskrinch_test_wait::PATIENCE`, which only keeps a
//! broken clock from hanging the run.
//!
//! That a change of zone is shown at once is claimed where time is the
//! test's to set: the ticker model ticks with the time the change was
//! heard (`ticker`'s `a_time_zone_change_ticks_in_the_new_zone`), and the
//! worker blocks on the zone watch rather than looking for it from time to
//! time (`clock`'s `a_zone_change_wakes_a_waiting_worker_by_itself`).

// Inside `cfg(test)`, where clippy allows the tests their `unwrap`s.
#[cfg(test)]
mod real_clock {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
    use std::time::Duration;

    use deskrinch_clock::jiff::{Timestamp, tz::TimeZone};
    use deskrinch_clock::{
        Cause, Clock, ClockError, Environment, Event, Precision, Tick, next_boundary,
    };

    use deskrinch_test_wait::PATIENCE;

    const ZONEINFO: &str = "/usr/share/zoneinfo";

    fn env(localtime: &Path) -> Environment {
        Environment {
            tz: None,
            zoneinfo: PathBuf::from(ZONEINFO),
            localtime: localtime.to_owned(),
            time_locale: None,
        }
    }

    fn start(env: Environment, precision: Precision) -> (Clock, Receiver<Event>) {
        let (send, events) = mpsc::channel();
        let clock = Clock::start(env, precision, move |event| {
            send.send(event).ok();
        })
        .unwrap();
        (clock, events)
    }

    /// The next event, within [`PATIENCE`].
    fn event(events: &Receiver<Event>) -> Event {
        events
            .recv_timeout(PATIENCE)
            .unwrap_or_else(|error| panic!("no event within {PATIENCE:?}: {error}"))
    }

    /// The next event, which must be a tick.
    fn tick(events: &Receiver<Event>) -> Tick {
        match event(events) {
            Event::Tick(tick) => tick,
            Event::Error(error) => panic!("expected a tick, got {error:?}"),
        }
    }

    /// The next tick of `cause`, the boundaries the clock reached
    /// meanwhile passed over: a slow machine may let the minute turn first.
    fn tick_of(events: &Receiver<Event>, cause: Cause) -> Tick {
        loop {
            let tick = tick(events);
            if tick.cause == cause {
                return tick;
            }
            assert_eq!(tick.cause, Cause::Boundary, "{tick:?}");
        }
    }

    /// Points `link` at `zone`, the way `timedatectl set-timezone` does: a new
    /// link renamed over the old one.
    fn set_zone(link: &Path, zone: &str) {
        let new = link.with_extension("new");
        symlink(format!("{ZONEINFO}/{zone}"), &new).unwrap();
        fs::rename(&new, link).unwrap();
    }

    #[test]
    fn starts_with_the_time_now_and_wakes_at_each_second() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        set_zone(&localtime, "Europe/Berlin");
        let before = Timestamp::now();
        let (_clock, events) = start(env(&localtime), Precision::Seconds);

        let first = tick(&events);
        assert_eq!(first.cause, Cause::Start);
        assert_eq!(first.now.time_zone().iana_name(), Some("Europe/Berlin"));
        let zone = first.now.time_zone().clone();
        let mut last = first.now.timestamp();
        // The time it is: not before the clock started, nor after the tick
        // was received.
        assert!(
            last >= before,
            "{last} is before the clock started, {before}"
        );
        assert!(last <= Timestamp::now(), "{last} is in the future");
        for _ in 0..3 {
            let boundary = next_boundary(last, &zone, Precision::Seconds);
            let next = tick(&events);
            let received = Timestamp::now();
            assert_eq!(next.cause, Cause::Boundary);
            let now = next.now.timestamp();
            // Never early: at the second it is for, or after it.
            assert!(now >= boundary, "woke at {now}, before {boundary}");
            // The time it is: not after the tick was received.
            assert!(
                now <= received,
                "{now} is after it was received, {received}"
            );
            // The second it is for, unless the machine woke the clock a
            // second or more late: then the time it is, a later second,
            // said here and not failed on.
            if now >= next_boundary(boundary, &zone, Precision::Seconds) {
                eprintln!(
                    "the machine woke the clock {:#} late for the second at {boundary}: \
                     it showed {now}",
                    now.duration_since(boundary)
                );
            }
            last = now;
        }
    }

    /// How long a minutes clock is watched for a tick it should not make:
    /// longer than the second a clock that wrongly woke at each second
    /// would take to tick, so that such a clock is seen.
    const SECONDS_TICK_WINDOW: Duration = Duration::from_secs(2);

    #[test]
    fn a_minutes_clock_sleeps_until_the_minute() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        set_zone(&localtime, "UTC");
        let (_clock, events) = start(env(&localtime), Precision::Minutes);
        let first = tick(&events);
        let minute = next_boundary(first.now.timestamp(), &TimeZone::UTC, Precision::Minutes);
        // Whatever comes within the window (nothing, unless the minute
        // turns meanwhile) is the minute's tick: a clock that woke at each
        // second would show a time before the minute. The window is one to
        // look in, not a bound on the clock.
        match events.recv_timeout(SECONDS_TICK_WINDOW) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(Event::Tick(tick)) => {
                assert_eq!(tick.cause, Cause::Boundary);
                assert!(
                    tick.now.timestamp() >= minute,
                    "woke at {}, before the minute at {minute}",
                    tick.now.timestamp()
                );
            }
            other => panic!("expected the minute or nothing, got {other:?}"),
        }
    }

    #[test]
    fn shows_a_time_zone_change() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        set_zone(&localtime, "Europe/Berlin");
        let (_clock, events) = start(env(&localtime), Precision::Minutes);
        let first = tick(&events);
        assert_eq!(first.now.time_zone().iana_name(), Some("Europe/Berlin"));

        let changed = Timestamp::now();
        set_zone(&localtime, "Asia/Tokyo");
        let tick = tick_of(&events, Cause::TimeZoneChanged);
        let received = Timestamp::now();
        assert_eq!(tick.now.time_zone().iana_name(), Some("Asia/Tokyo"));
        assert_eq!(tick.now.offset().seconds(), 9 * 3600);
        // The time it is: not before the change, nor after it was received.
        let now = tick.now.timestamp();
        assert!(now >= changed, "{now} is before the change, {changed}");
        assert!(
            now <= received,
            "{now} is after it was received, {received}"
        );
    }

    #[test]
    fn shows_utc_until_an_unusable_zone_is_fixed() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        fs::write(&localtime, "not a time zone").unwrap();
        let (_clock, events) = start(env(&localtime), Precision::Minutes);
        match event(&events) {
            Event::Error(ClockError::TimeZone(_)) => {}
            other => panic!("expected the zone's error, got {other:?}"),
        }
        let first = tick(&events);
        assert_eq!(first.cause, Cause::Start);
        assert_eq!(first.now.time_zone(), &TimeZone::UTC);

        set_zone(&localtime, "America/New_York");
        let fixed = tick_of(&events, Cause::TimeZoneChanged);
        assert_eq!(fixed.now.time_zone().iana_name(), Some("America/New_York"));
    }

    #[test]
    fn a_drop_ends_the_clocks_thread() {
        let dir = tempfile::tempdir().unwrap();
        let (clock, events) = start(env(&dir.path().join("localtime")), Precision::Seconds);
        tick(&events);
        // A drop stops the thread and joins it: once it returns, the thread
        // and the subscriber it owned are gone. What it sent before is
        // there to read, and then the channel is closed, with nothing to
        // wait for.
        drop(clock);
        loop {
            match events.try_recv() {
                Ok(Event::Tick(_)) => {}
                Ok(other) => panic!("{other:?}"),
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => panic!("the clock's thread outlived the drop"),
            }
        }
    }
}

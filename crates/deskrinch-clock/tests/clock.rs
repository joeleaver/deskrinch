//! The clock against the real wall clock and real time zone files.
//!
//! Setting the wall clock needs `CAP_SYS_TIME`, which a test does not have;
//! the clock's response to that is tested on the model (`ticker`) with a
//! clock the test controls. Here: the timer wakes at the boundary, the time
//! zone file is watched, and the clock stops when dropped.
//!
//! These tests assert what the clock promises, and nothing about how soon
//! the machine runs a woken thread: it never wakes before the boundary,
//! each wake reports the time it is, and a change it watches for is
//! reported. A wake the machine made late is reported as late (with the
//! time it is, a later second when it is late by more than one), and
//! the test says so on its output without failing. How late the clock
//! wakes in practice is a measurement (`cargo xtask measure`), not an
//! assertion. Every wait here has [`PATIENCE`], which only keeps a broken
//! clock from hanging the run.

// Inside `cfg(test)`, where clippy allows the tests their `unwrap`s.
#[cfg(test)]
mod real_clock {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
    use std::time::Duration;

    use deskrinch_clock::jiff::{Timestamp, tz::TimeZone};
    use deskrinch_clock::{
        Cause, Clock, ClockError, Environment, Event, Precision, Tick, next_boundary,
    };

    const ZONEINFO: &str = "/usr/share/zoneinfo";

    /// How long a wait may take before the test calls the clock hung: far
    /// beyond any boundary these tests wait for, and no bound on how late
    /// the machine wakes a thread.
    const PATIENCE: Duration = Duration::from_secs(120);

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

    /// Checks that `tick`, received at `received` after the clock last
    /// reported `previous`, reports the time it is: after `previous`, and
    /// not after it was received.
    fn reports_the_time_it_is(tick: &Tick, previous: Timestamp, received: Timestamp) {
        let now = tick.now.timestamp();
        assert!(now > previous, "{now} is not after {previous}");
        assert!(
            now <= received,
            "{now} is after it was received, {received}"
        );
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
        assert!(
            last >= before,
            "{last} is before the clock started, {before}"
        );
        for _ in 0..3 {
            let boundary = next_boundary(last, &zone, Precision::Seconds);
            let next = tick(&events);
            let received = Timestamp::now();
            assert_eq!(next.cause, Cause::Boundary);
            reports_the_time_it_is(&next, last, received);
            let now = next.now.timestamp();
            // Never early: at the boundary or after it.
            assert!(now >= boundary, "woke at {now}, before {boundary}");
            // The next second, unless the machine woke it late by more than
            // one: then the time it is, a later second, which is said here.
            if now >= next_boundary(boundary, &zone, Precision::Seconds) {
                eprintln!(
                    "the machine woke the clock {} late: it reported {now} for the second at \
                     {boundary}",
                    now.duration_since(boundary)
                );
            }
            last = now;
        }
    }

    #[test]
    fn a_minutes_clock_sleeps_until_the_minute() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        set_zone(&localtime, "UTC");
        let (_clock, events) = start(env(&localtime), Precision::Minutes);
        let first = tick(&events);
        let minute = next_boundary(first.now.timestamp(), &TimeZone::UTC, Precision::Minutes);
        // Whatever arrives in the next two seconds (nothing, unless the
        // minute turns meanwhile) is the minute's tick: a clock that woke
        // at each second would report one before it.
        match events.recv_timeout(Duration::from_secs(2)) {
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
    fn hears_a_time_zone_change() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        set_zone(&localtime, "Europe/Berlin");
        let (_clock, events) = start(env(&localtime), Precision::Minutes);
        let first = tick(&events);
        assert_eq!(first.now.time_zone().iana_name(), Some("Europe/Berlin"));

        set_zone(&localtime, "Asia/Tokyo");
        let tick = tick_of(&events, Cause::TimeZoneChanged);
        assert_eq!(tick.now.time_zone().iana_name(), Some("Asia/Tokyo"));
        assert_eq!(tick.now.offset().seconds(), 9 * 3600);
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
    fn stops_when_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (clock, events) = start(env(&dir.path().join("localtime")), Precision::Seconds);
        tick(&events);
        drop(clock);
        // Dropping it ended its thread, and the subscriber with it: what it
        // sent before is there to read, then nothing more ever comes.
        loop {
            match events.recv_timeout(PATIENCE) {
                Ok(Event::Tick(_)) => {}
                Ok(other) => panic!("{other:?}"),
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => panic!("the clock ran on after it was dropped"),
            }
        }
    }
}

//! The clock against the real wall clock and real time zone files.
//!
//! Setting the wall clock needs `CAP_SYS_TIME`, which a test does not have;
//! the clock's response to that is tested on the model (`ticker`) with a
//! clock the test controls. Here: the timer wakes at the boundary, the time
//! zone file is watched, and the clock stops when dropped.

// Inside `cfg(test)`, where clippy allows the tests their `unwrap`s.
#[cfg(test)]
mod real_clock {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{self, Receiver};
    use std::time::{Duration, Instant};

    use deskrinch_clock::jiff::{SignedDuration, Timestamp, tz::TimeZone};
    use deskrinch_clock::{
        Cause, Clock, ClockError, Environment, Event, Precision, Tick, next_boundary,
    };

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

    fn tick(events: &Receiver<Event>, within: Duration) -> Tick {
        match events.recv_timeout(within) {
            Ok(Event::Tick(tick)) => tick,
            other => panic!("expected a tick, got {other:?}"),
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
        let (_clock, events) = start(env(&localtime), Precision::Seconds);

        let first = tick(&events, Duration::from_secs(1));
        assert_eq!(first.cause, Cause::Start);
        assert_eq!(first.now.time_zone().iana_name(), Some("Europe/Berlin"));
        let mut expected = first.now.timestamp();
        for _ in 0..2 {
            expected = next_boundary(expected, first.now.time_zone(), Precision::Seconds);
            let next = tick(&events, Duration::from_millis(1500));
            assert_eq!(next.cause, Cause::Boundary);
            // On time: at the boundary, not before, and promptly after.
            let late = next.now.timestamp().duration_since(expected);
            assert!(!late.is_negative(), "{late:?} early");
            assert!(late.as_millis() < 100, "{late:?} late");
        }
    }

    #[test]
    fn a_minutes_clock_sleeps_until_the_minute() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        set_zone(&localtime, "UTC");
        let (_clock, events) = start(env(&localtime), Precision::Minutes);
        let first = tick(&events, Duration::from_secs(1));
        let until = next_boundary(first.now.timestamp(), &TimeZone::UTC, Precision::Minutes);
        let quiet = until.duration_since(Timestamp::now()) - SignedDuration::from_millis(200);
        if let Ok(quiet) = Duration::try_from(quiet) {
            assert!(
                events
                    .recv_timeout(quiet.min(Duration::from_secs(2)))
                    .is_err(),
                "woke before the minute"
            );
        }
    }

    #[test]
    fn hears_a_time_zone_change_within_a_second() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        set_zone(&localtime, "Europe/Berlin");
        let (_clock, events) = start(env(&localtime), Precision::Minutes);
        let first = tick(&events, Duration::from_secs(1));
        assert_eq!(first.now.time_zone().iana_name(), Some("Europe/Berlin"));

        set_zone(&localtime, "Asia/Tokyo");
        let changed = Instant::now();
        let tick = tick(&events, Duration::from_secs(1));
        assert!(changed.elapsed() < Duration::from_secs(1));
        assert_eq!(tick.cause, Cause::TimeZoneChanged);
        assert_eq!(tick.now.time_zone().iana_name(), Some("Asia/Tokyo"));
        assert_eq!(tick.now.offset().seconds(), 9 * 3600);
    }

    #[test]
    fn shows_utc_until_an_unusable_zone_is_fixed() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        fs::write(&localtime, "not a time zone").unwrap();
        let (_clock, events) = start(env(&localtime), Precision::Minutes);
        match events.recv_timeout(Duration::from_secs(1)) {
            Ok(Event::Error(ClockError::TimeZone(_))) => {}
            other => panic!("expected the zone's error, got {other:?}"),
        }
        let first = tick(&events, Duration::from_secs(1));
        assert_eq!(first.now.time_zone(), &TimeZone::UTC);

        set_zone(&localtime, "America/New_York");
        let fixed = tick(&events, Duration::from_secs(1));
        assert_eq!(fixed.cause, Cause::TimeZoneChanged);
        assert_eq!(fixed.now.time_zone().iana_name(), Some("America/New_York"));
    }

    #[test]
    fn stops_when_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (clock, events) = start(env(&dir.path().join("localtime")), Precision::Seconds);
        tick(&events, Duration::from_secs(1));
        let dropping = Instant::now();
        drop(clock);
        assert!(dropping.elapsed() < Duration::from_millis(200));
        // The subscriber went with the thread.
        while let Ok(event) = events.recv_timeout(Duration::from_millis(1100)) {
            assert!(matches!(event, Event::Tick(_)), "{event:?}");
        }
    }
}

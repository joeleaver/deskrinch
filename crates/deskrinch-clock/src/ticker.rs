//! The clock's decisions, apart from the timers and files that drive them:
//! given the time it is now, whether the displayed time changed and when it
//! next will. The worker thread feeds it real time; the tests feed it any.

use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};

use crate::boundary::{Precision, next_boundary, shown};

/// Why the subscriber hears the time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cause {
    /// The clock started: the first time it reports.
    Start,
    /// The displayed time reached its next minute (or second), or the time
    /// zone's offset changed on schedule, as at a daylight-saving
    /// transition.
    Boundary,
    /// The wall clock was set, stepped by NTP or resumed from suspend, and
    /// the displayed time is now different.
    ClockChanged,
    /// The system time zone changed. The displayed time may be the same, as
    /// when moving between zones with the same offset.
    TimeZoneChanged,
}

/// The time now, and why it is reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    /// The time, in the current time zone.
    pub now: Zoned,
    /// Why it is reported.
    pub cause: Cause,
}

/// What the clock shows, and the instant it last looked.
#[derive(Debug)]
pub(crate) struct Ticker {
    zone: TimeZone,
    precision: Precision,
    /// The displayed time, as [`shown`] numbers it.
    shown: i64,
    /// When the clock last looked at the time.
    now: Timestamp,
}

impl Ticker {
    /// A clock showing `now` in `zone`, and its first tick.
    pub(crate) fn start(zone: TimeZone, precision: Precision, now: Timestamp) -> (Self, Tick) {
        let mut ticker = Self {
            shown: shown(now, &zone, precision),
            zone,
            precision,
            now,
        };
        let tick = ticker.tick(Cause::Start);
        (ticker, tick)
    }

    /// The instant the displayed time next changes, if the wall clock runs
    /// on undisturbed.
    pub(crate) fn deadline(&self) -> Timestamp {
        next_boundary(self.now, &self.zone, self.precision)
    }

    /// The wall clock reads `now`, because the deadline passed
    /// ([`Cause::Boundary`]) or the clock was set ([`Cause::ClockChanged`]).
    /// A tick when the displayed time is different.
    pub(crate) fn observe(&mut self, now: Timestamp, cause: Cause) -> Option<Tick> {
        self.now = now;
        let shown = shown(now, &self.zone, self.precision);
        (shown != self.shown).then(|| {
            self.shown = shown;
            self.tick(cause)
        })
    }

    /// The time zone is now `zone`. A tick when it is a different zone.
    pub(crate) fn change_zone(&mut self, zone: TimeZone, now: Timestamp) -> Option<Tick> {
        if zone == self.zone {
            return self.observe(now, Cause::ClockChanged);
        }
        self.zone = zone;
        self.now = now;
        self.shown = shown(now, &self.zone, self.precision);
        Some(self.tick(Cause::TimeZoneChanged))
    }

    fn tick(&mut self, cause: Cause) -> Tick {
        Tick {
            now: self.now.to_zoned(self.zone.clone()),
            cause,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_zones::{named, ts};

    fn local(tick: &Tick) -> String {
        tick.now.datetime().to_string()
    }

    #[test]
    fn ticks_once_per_minute_at_the_boundary() {
        let (mut ticker, first) = Ticker::start(
            TimeZone::UTC,
            Precision::Minutes,
            ts("2026-11-10T12:00:30Z"),
        );
        assert_eq!(first.cause, Cause::Start);
        assert_eq!(ticker.deadline(), ts("2026-11-10T12:01:00Z"));
        // A wake inside the minute shows nothing new.
        assert_eq!(
            ticker.observe(ts("2026-11-10T12:00:59.9Z"), Cause::Boundary),
            None
        );
        let tick = ticker
            .observe(ts("2026-11-10T12:01:00.002Z"), Cause::Boundary)
            .unwrap();
        assert_eq!(local(&tick), "2026-11-10T12:01:00.002");
        assert_eq!(tick.cause, Cause::Boundary);
        assert_eq!(ticker.deadline(), ts("2026-11-10T12:02:00Z"));
    }

    #[test]
    fn a_clock_set_forward_ticks_and_rearms() {
        // A resume from a night's suspend, or a manual change.
        let (mut ticker, _) = Ticker::start(
            TimeZone::UTC,
            Precision::Minutes,
            ts("2026-11-10T23:10:15Z"),
        );
        let tick = ticker
            .observe(ts("2026-11-11T07:42:05Z"), Cause::ClockChanged)
            .unwrap();
        assert_eq!(tick.cause, Cause::ClockChanged);
        assert_eq!(local(&tick), "2026-11-11T07:42:05");
        assert_eq!(ticker.deadline(), ts("2026-11-11T07:43:00Z"));
    }

    #[test]
    fn a_clock_set_back_ticks_and_rearms_earlier() {
        // An NTP step back of two minutes.
        let (mut ticker, _) = Ticker::start(
            TimeZone::UTC,
            Precision::Minutes,
            ts("2026-11-10T12:05:10Z"),
        );
        let tick = ticker
            .observe(ts("2026-11-10T12:03:10Z"), Cause::ClockChanged)
            .unwrap();
        assert_eq!(local(&tick), "2026-11-10T12:03:10");
        assert_eq!(ticker.deadline(), ts("2026-11-10T12:04:00Z"));
    }

    #[test]
    fn a_small_step_inside_the_minute_only_rearms() {
        let (mut ticker, _) = Ticker::start(
            TimeZone::UTC,
            Precision::Minutes,
            ts("2026-11-10T12:05:10Z"),
        );
        assert_eq!(
            ticker.observe(ts("2026-11-10T12:05:09.7Z"), Cause::ClockChanged),
            None
        );
        assert_eq!(ticker.deadline(), ts("2026-11-10T12:06:00Z"));
        // For a seconds clock the same step is a change.
        let (mut ticker, _) = Ticker::start(
            TimeZone::UTC,
            Precision::Seconds,
            ts("2026-11-10T12:05:10Z"),
        );
        assert!(
            ticker
                .observe(ts("2026-11-10T12:05:09.7Z"), Cause::ClockChanged)
                .is_some()
        );
    }

    #[test]
    fn a_time_zone_change_ticks_in_the_new_zone() {
        let at = ts("2026-11-10T13:05:00Z");
        let (mut ticker, first) = Ticker::start(named("Europe/Berlin"), Precision::Minutes, at);
        assert_eq!(local(&first), "2026-11-10T14:05:00");
        // At once: a change the clock hears inside the minute ticks with
        // the time it heard it, not at the next boundary.
        let heard = ts("2026-11-10T13:05:20.5Z");
        let tick = ticker.change_zone(named("Asia/Tokyo"), heard).unwrap();
        assert_eq!(tick.cause, Cause::TimeZoneChanged);
        assert_eq!(tick.now.timestamp(), heard);
        assert_eq!(local(&tick), "2026-11-10T22:05:20.5");
        // The same offset under another name is still a change of zone.
        let tick = ticker.change_zone(named("Asia/Seoul"), at).unwrap();
        assert_eq!(local(&tick), "2026-11-10T22:05:00");
        // Reading the same zone again is not.
        assert_eq!(ticker.change_zone(named("Asia/Seoul"), at), None);
    }

    #[test]
    fn ticks_across_a_daylight_saving_transition() {
        let new_york = named("America/New_York");
        let (mut ticker, _) =
            Ticker::start(new_york, Precision::Minutes, ts("2026-11-01T05:59:30Z"));
        let deadline = ticker.deadline();
        let tick = ticker.observe(deadline, Cause::Boundary).unwrap();
        assert_eq!(
            tick.now.to_string(),
            "2026-11-01T01:00:00-05:00[America/New_York]"
        );
    }
}

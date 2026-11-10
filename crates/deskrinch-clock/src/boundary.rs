//! When the displayed time next changes.
//!
//! A clock showing minutes changes when the local time's minute rolls over,
//! and also whenever the time zone's offset changes (a daylight-saving
//! transition, or a historical change of standard time). Both are found
//! here on absolute timestamps, never by adding a minute to a civil time:
//! at the end of daylight saving time the civil time after `01:59` is
//! `01:00`, not `02:00`.
//!
//! Timestamps are POSIX time, which has no leap seconds: a minute is always
//! 60 timestamp seconds. When the kernel inserts a leap second it repeats
//! the last second of the minute, so the wall clock reaches the next minute
//! one SI second later and the boundary computed here is still the instant
//! the displayed time changes.

use jiff::Timestamp;
use jiff::tz::TimeZone;

/// The smallest unit of time a clock shows, and so how often it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Precision {
    /// Hours and minutes: the clock changes once a minute.
    Minutes,
    /// Hours, minutes and seconds: the clock changes once a second.
    Seconds,
}

impl Precision {
    /// The unit's length in seconds.
    const fn seconds(self) -> i64 {
        match self {
            Self::Minutes => 60,
            Self::Seconds => 1,
        }
    }
}

/// The first instant after `now` at which a clock in `zone` showing time to
/// `precision` shows something different: the next rollover of the local
/// minute (or second), or an earlier change of the zone's offset.
///
/// The result is a whole second, and strictly after `now`.
#[must_use]
pub fn next_boundary(now: Timestamp, zone: &TimeZone, precision: Precision) -> Timestamp {
    let unit = precision.seconds();
    let offset = zone.to_offset(now);
    let second = whole_second(now);
    let local = second + i64::from(offset.seconds());
    let rollover = second - local.rem_euclid(unit) + unit;
    let transition = zone
        .following(now)
        .map(|transition| (whole_second(transition.timestamp()), transition.offset()))
        .take_while(|&(at, _)| at < rollover)
        .find(|&(at, new)| at > second && new != offset)
        .map(|(at, _)| at);
    let next = transition.unwrap_or(rollover);
    // In range: `next` is within a minute of `now`, which jiff accepts.
    Timestamp::from_second(next).unwrap_or(Timestamp::MAX)
}

/// What a clock in `zone` showing time to `precision` displays at `now`, as
/// a number: the local time in whole units since the epoch. Two instants
/// show the same thing exactly when their numbers are equal.
pub(crate) fn shown(now: Timestamp, zone: &TimeZone, precision: Precision) -> i64 {
    let local = whole_second(now) + i64::from(zone.to_offset(now).seconds());
    local.div_euclid(precision.seconds())
}

/// The whole second `at` falls in, rounding towards the past (jiff
/// truncates towards the epoch).
fn whole_second(at: Timestamp) -> i64 {
    let second = at.as_second();
    if at.subsec_nanosecond() < 0 {
        second - 1
    } else {
        second
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_zones::{named, ts};

    fn next(now: &str, zone: &TimeZone, precision: Precision) -> String {
        next_boundary(ts(now), zone, precision).to_string()
    }

    #[test]
    fn wakes_at_the_next_minute_or_second() {
        let utc = TimeZone::UTC;
        assert_eq!(
            next("2026-11-10T12:34:56.5Z", &utc, Precision::Minutes),
            "2026-11-10T12:35:00Z"
        );
        assert_eq!(
            next("2026-11-10T12:34:56.5Z", &utc, Precision::Seconds),
            "2026-11-10T12:34:57Z"
        );
        // Exactly on a boundary, the next one is a whole unit later.
        assert_eq!(
            next("2026-11-10T12:35:00Z", &utc, Precision::Minutes),
            "2026-11-10T12:36:00Z"
        );
        assert_eq!(
            next("1969-12-31T23:59:59.5Z", &utc, Precision::Seconds),
            "1970-01-01T00:00:00Z"
        );
    }

    #[test]
    fn follows_local_minutes_under_an_offset_of_odd_seconds() {
        // Amsterdam kept +00:19:32 until 1937: its minutes turned over 28
        // seconds past each UTC minute.
        let amsterdam = named("Europe/Amsterdam");
        assert_eq!(
            next("1930-06-01T10:00:00Z", &amsterdam, Precision::Minutes),
            "1930-06-01T10:00:28Z"
        );
    }

    #[test]
    fn spring_forward_skips_the_missing_hour() {
        // At 02:00 EST, New York's clocks go to 03:00 EDT.
        let new_york = TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").unwrap();
        let at = ts("2026-03-08T06:59:30Z"); // 01:59:30 EST
        let boundary = next_boundary(at, &new_york, Precision::Minutes);
        assert_eq!(boundary, ts("2026-03-08T07:00:00Z"));
        assert_eq!(
            boundary.to_zoned(new_york).datetime().to_string(),
            "2026-03-08T03:00:00"
        );
    }

    #[test]
    fn fall_back_repeats_the_hour() {
        // At 02:00 EDT, the clocks go back to 01:00 EST: the boundary after
        // 01:59:30 EDT is thirty seconds later, showing 01:00, not an hour
        // and a half later at 02:00 EST.
        let new_york = named("America/New_York");
        let at = ts("2026-11-01T05:59:30Z"); // 01:59:30 EDT
        let boundary = next_boundary(at, &new_york, Precision::Minutes);
        assert_eq!(boundary, ts("2026-11-01T06:00:00Z"));
        assert_eq!(
            boundary.to_zoned(new_york.clone()).datetime().to_string(),
            "2026-11-01T01:00:00"
        );
        assert_ne!(
            shown(at, &new_york, Precision::Minutes),
            shown(boundary, &new_york, Precision::Minutes)
        );
    }

    #[test]
    fn a_transition_off_the_minute_is_a_boundary_of_its_own() {
        // Amsterdam left +00:19:32 for +00:20 at 1937-07-01 00:00 local,
        // 23:40:28 UTC: the clock jumped from 00:00:00 to 00:00:28, inside
        // a minute, and a seconds clock changes there.
        let amsterdam = named("Europe/Amsterdam");
        let at = ts("1937-06-30T23:40:27.5Z");
        assert_eq!(
            next_boundary(at, &amsterdam, Precision::Seconds),
            ts("1937-06-30T23:40:28Z")
        );
    }

    #[test]
    fn a_transition_that_keeps_the_offset_is_not_a_boundary() {
        // A rule whose daylight time has the standard offset: its
        // transitions rename the zone without moving the clock.
        let flat = TimeZone::posix("AAA-1BBB-1,M3.5.0,M10.5.0/3").unwrap();
        assert_eq!(
            next("2026-03-29T00:59:30Z", &flat, Precision::Minutes),
            "2026-03-29T01:00:00Z"
        );
        assert_eq!(
            next("2026-03-29T00:59:30Z", &flat, Precision::Seconds),
            "2026-03-29T00:59:31Z"
        );
    }

    #[test]
    fn a_leap_second_does_not_add_a_boundary() {
        // 2016-12-31 ended with 23:59:60 UTC. POSIX time has no such
        // second, so the next minute is still 2017-01-01T00:00:00Z.
        let utc = TimeZone::UTC;
        assert_eq!(
            next("2016-12-31T23:59:59.9Z", &utc, Precision::Minutes),
            "2017-01-01T00:00:00Z"
        );
        // The kernel inserts it by stepping back to 23:59:59: the same
        // boundary is computed again, and the display has not changed.
        let before = ts("2016-12-31T23:59:59.9Z");
        let repeated = ts("2016-12-31T23:59:59.1Z");
        assert_eq!(
            next_boundary(repeated, &utc, Precision::Minutes),
            next_boundary(before, &utc, Precision::Minutes)
        );
        assert_eq!(
            shown(repeated, &utc, Precision::Seconds),
            shown(before, &utc, Precision::Seconds)
        );
    }

    #[test]
    fn shows_the_same_minute_on_both_sides_of_a_fold_only_by_its_number() {
        let new_york = named("America/New_York");
        let edt = ts("2026-11-01T05:30:00Z"); // 01:30 EDT
        let est = ts("2026-11-01T06:30:00Z"); // 01:30 EST
        assert_eq!(
            shown(edt, &new_york, Precision::Minutes),
            shown(est, &new_york, Precision::Minutes)
        );
    }
}

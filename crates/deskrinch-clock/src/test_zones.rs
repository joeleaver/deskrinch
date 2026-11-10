//! Time zones and timestamps for the unit tests.
//!
//! Named zones are read from the system's tzdata, the files the crate reads
//! in use; POSIX rules stand in where a test needs no history.

use jiff::Timestamp;
use jiff::tz::TimeZone;

/// Where the tests find the system's time zone files.
pub(crate) const SYSTEM_ZONEINFO: &str = "/usr/share/zoneinfo";

/// The IANA zone `name`, from the system's tzdata.
pub(crate) fn named(name: &str) -> TimeZone {
    let path = format!("{SYSTEM_ZONEINFO}/{name}");
    let data = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    TimeZone::tzif(name, &data).unwrap()
}

/// The instant an RFC 3339 timestamp names.
pub(crate) fn ts(text: &str) -> Timestamp {
    text.parse().unwrap()
}

//! Wall-clock time for the desktop: knowing when the time a clock shows
//! changes, and writing it the way the user's locale writes it.
//!
//! # Model
//!
//! - An [`Environment`] says where the time zone comes from (`TZ`, the
//!   zoneinfo directory, `/etc/localtime`) and which locale formats times
//!   (`LC_ALL`, `LC_TIME`, `LANG`). It is a value the caller builds,
//!   usually with [`Environment::from_process`]; nothing below it reads the
//!   process environment.
//! - [`next_boundary`] is the instant the displayed time next changes, at a
//!   [`Precision`] of minutes or seconds: the local minute rolling over, or
//!   an earlier change of the zone's offset.
//! - A [`Clock`] runs on a thread of its own and calls a subscriber with a
//!   [`Tick`] whenever the displayed time changes, saying why ([`Cause`]).
//! - A [`Formatter`] writes a time in a locale's own pattern, chosen by a
//!   [`Style`]; [`posix_to_unicode`] turns a POSIX locale name into the
//!   Unicode locale it uses.
//!
//! The boundary arithmetic, the clock's decisions, locale resolution and
//! formatting are pure and take the time as an argument; only [`Clock`]
//! reads the real clock and the time zone files. No function needs an async
//! runtime.
//!
//! # Waking at the right time
//!
//! A clock showing hours and minutes wakes once a minute, at the instant
//! the minute turns over, and at no other time. It also reacts to each
//! event that breaks a naive timer as soon as the kernel reports it,
//! without waiting for its next boundary: its thread blocks on the timer,
//! the zone watch and its stop signal together, and wakes for whichever
//! the kernel makes ready. How soon that is on a given machine is the
//! scheduler's doing, not the clock's (a measurement, not a promise):
//!
//! | Event | What a monotonic sleep does | What this clock does |
//! |---|---|---|
//! | Resume from suspend | wakes late by the time suspended (`CLOCK_MONOTONIC` stops during suspend) | the expiry passed during suspend, so the timer fires on resume, and the kernel cancels it as the clock catches up |
//! | Manual change (`date -s`, `timedatectl set-time`) | wakes at the old schedule | cancelled on set: reads the clock at once |
//! | NTP step (`adjtimex(ADJ_SETOFFSET)`) | wakes at the old schedule | cancelled on set |
//! | NTP slew | drifts from the minute | the expiry is on the wall clock, so it follows |
//! | Leap second | — | the kernel repeats the second and cancels on set; the boundary is unchanged, since POSIX time has no leap seconds |
//! | Daylight-saving transition | — | the transition is itself a boundary |
//! | Time zone change (`timedatectl set-timezone`) | — | inotify on `/etc/localtime` and the file it links to: reads the new zone at once |
//!
//! The timer is a `timerfd` on `CLOCK_REALTIME` armed with an absolute
//! expiry and `TFD_TIMER_CANCEL_ON_SET` (see the `timer` module for the
//! details, and `timerfd_create(2)`). `CLOCK_REALTIME_ALARM` would also
//! wake a suspended machine, which a clock on screen has no business doing.
//! Polling the clock every second would catch each event within a second
//! of its time slice, but would wake 60 times as often.
//!
//! # Formatting
//!
//! Times are formatted with CLDR's patterns through ICU4X, for the locale
//! named by `LC_ALL`, `LC_TIME` or `LANG`: the locale chooses 12- or
//! 24-hour time, the separators, the digits and the calendar. glibc's own
//! `LC_TIME` data offers no hours-and-minutes format (`%X` includes the
//! seconds), which is why CLDR, which KDE and GNOME also use, is the
//! source. The time zone and its rules are the system's: the TZif files the
//! environment names, read with jiff.
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_clock::{Clock, Environment, Event, Formatter, Precision, Style};
//!
//! let env = Environment::from_process();
//! let format = Formatter::new(&env.locale(), Style::Time(Precision::Minutes))?;
//! let clock = Clock::start(env, Precision::Minutes, move |event| match event {
//!     Event::Tick(tick) => println!("{}", format.format(&tick.now)),
//!     Event::Error(error) => eprintln!("clock: {error}"),
//! })?;
//! # drop(clock);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod boundary;
mod clock;
mod env;
mod error;
mod format;
mod locale;
#[cfg(test)]
mod test_patience;
#[cfg(test)]
mod test_zones;
mod ticker;
mod timer;
mod watch;
mod zone;

pub use boundary::{Precision, next_boundary};
pub use clock::{Clock, Event};
pub use env::Environment;
pub use error::{ClockError, FormatError, TimeZoneError};
pub use format::{Formatter, Length, Style};
pub use locale::posix_to_unicode;
pub use ticker::{Cause, Tick};

/// The Unicode locale type [`Formatter`] takes.
pub use icu_locale_core::Locale;
/// The date and time library this crate's API speaks, re-exported so a
/// caller uses the same version.
pub use jiff;

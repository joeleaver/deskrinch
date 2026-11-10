//! The clock: hours and minutes in the user's locale, changing on the
//! minute.
//!
//! A [`deskrinch_clock::Clock`] wakes on its own thread when the minute
//! turns over, the wall clock is set, the machine resumes or the time zone
//! changes, and the text is formatted there. It reaches the UI through
//! [`Signal::send`], rinch's way of writing a signal from another thread:
//! the write is dispatched to the main thread, where the text node's effect
//! updates that one node. The clock stops with the widget.

use std::error::Error;

use deskrinch_clock::{
    Clock, ClockError, Environment, Event, FormatError, Formatter, Precision, Style,
};
use rinch::prelude::*;

/// What the clock shows for `event`: the new text for a tick, nothing for
/// an error (which is logged).
fn text_for(event: Event, format: &Formatter) -> Option<String> {
    match event {
        Event::Tick(tick) => Some(format.format(&tick.now)),
        Event::Error(error) => {
            log("clock", &error);
            None
        }
    }
}

/// Starts the clock for `env`, writing its text to `text`.
fn start(env: Environment, text: Signal<String>) -> Result<Clock, StartError> {
    let format = Formatter::new(&env.locale(), Style::Time(Precision::Minutes))?;
    let clock = Clock::start(env, Precision::Minutes, move |event| {
        if let Some(shown) = text_for(event, &format) {
            text.send(shown);
        }
    })?;
    Ok(clock)
}

/// Why the clock could not start.
#[derive(Debug, thiserror::Error)]
enum StartError {
    #[error("cannot load the time format")]
    Format(#[from] FormatError),
    #[error("cannot start the clock")]
    Clock(#[from] ClockError),
}

/// Writes `error` and its causes to standard error, for the journal.
fn log(what: &str, error: &dyn Error) {
    let mut message = format!("deskrinch-panel: {what}: {error}");
    let mut cause = error.source();
    while let Some(inner) = cause {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        cause = inner.source();
    }
    eprintln!("{message}");
}

/// The clock widget. Without a clock (it could not start, which is logged)
/// it is empty.
#[component]
pub(crate) fn ClockWidget() -> NodeHandle {
    let text = Signal::new(String::new());
    match start(Environment::from_process(), text) {
        Ok(clock) => {
            rinch::reactive::on_cleanup(move || drop(clock));
        }
        Err(error) => log("clock", &error),
    }
    rsx! {
        span {
            class: "clock",
            style: "white-space: nowrap; font-variant-numeric: tabular-nums;",
            {|| text.get()}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskrinch_clock::jiff::Timestamp;
    use deskrinch_clock::jiff::tz::TimeZone;
    use deskrinch_clock::{Cause, Tick, posix_to_unicode};

    fn tick(at: &str, zone: &str) -> Event {
        let now: Timestamp = at.parse().unwrap();
        let zone = TimeZone::posix(zone).unwrap();
        Event::Tick(Tick {
            now: now.to_zoned(zone),
            cause: Cause::Boundary,
        })
    }

    #[test]
    fn shows_hours_and_minutes_in_the_locale() {
        let style = Style::Time(Precision::Minutes);
        let german = Formatter::new(&posix_to_unicode("de_DE.UTF-8"), style).unwrap();
        let american = Formatter::new(&posix_to_unicode("en_US.UTF-8"), style).unwrap();
        let at = "2026-11-10T13:05:09Z";
        assert_eq!(
            text_for(tick(at, "CET-1"), &german).as_deref(),
            Some("14:05")
        );
        assert_eq!(
            text_for(tick(at, "CET-1"), &american).as_deref(),
            Some("2:05\u{202f}PM")
        );
    }

    #[test]
    fn an_error_leaves_the_text_alone() {
        let format =
            Formatter::new(&posix_to_unicode("C"), Style::Time(Precision::Minutes)).unwrap();
        let error = ClockError::Watch(std::io::Error::other("no inotify"));
        assert_eq!(text_for(Event::Error(error), &format), None);
    }
}

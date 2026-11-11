//! The clock: the time (and, if set, the date) in the user's locale,
//! changing on the minute, or on the second when it shows seconds.
//!
//! A [`deskrinch_clock::Clock`] wakes on its own thread when the displayed
//! time changes, the wall clock is set, the machine resumes or the time
//! zone changes, and the text is formatted there. It reaches the UI through
//! [`Signal::send`], rinch's way of writing a signal from another thread:
//! the write is dispatched to the main thread, where the text node's effect
//! updates that one node.
//!
//! The clock follows the `[panel.clock]` settings: when they change, the
//! running clock is stopped and a new one started at the new precision and
//! style. It stops with the widget.

use std::cell::RefCell;
use std::rc::Rc;

use deskrinch_clock::{
    Clock, ClockError, Environment, Event, FormatError, Formatter, Length, Precision, Style,
};
use deskrinch_component::report;
use deskrinch_settings::panel::{ClockSettings, PanelSettings};
use rinch::prelude::*;
use rinch::reactive::Effect;

/// How often a clock with `settings` changes.
fn precision(settings: ClockSettings) -> Precision {
    if settings.seconds {
        Precision::Seconds
    } else {
        Precision::Minutes
    }
}

/// How a clock with `settings` is written, in the user's locale: the time,
/// or the date (medium length: "10 Nov 2026") and the time.
fn style(settings: ClockSettings) -> Style {
    if settings.date {
        Style::DateTime(Length::Medium, precision(settings))
    } else {
        Style::Time(precision(settings))
    }
}

/// What the clock shows for `event`: the new text for a tick, nothing for
/// an error (which is logged).
fn text_for(event: Event, format: &Formatter) -> Option<String> {
    match event {
        Event::Tick(tick) => Some(format.format(&tick.now)),
        Event::Error(error) => {
            report::error("clock", &error);
            None
        }
    }
}

/// Starts a clock for `env` as `settings` say, writing its text to `text`.
fn start(
    env: Environment,
    settings: ClockSettings,
    text: Signal<String>,
) -> Result<Clock, StartError> {
    let format = Formatter::new(&env.locale(), style(settings))?;
    let clock = Clock::start(env, precision(settings), move |event| {
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

/// The clock widget, following the panel's settings (the context's
/// `Signal<PanelSettings>`). Without a clock (it could not start, which is
/// logged) it is empty.
#[component]
pub(crate) fn ClockWidget() -> NodeHandle {
    let panel = use_context::<Signal<PanelSettings>>();
    let settings = Memo::new(move || panel.get().clock);
    let text = Signal::new(String::new());
    let env = Environment::from_process();
    let running: Rc<RefCell<Option<Clock>>> = Rc::default();
    let follow = Rc::clone(&running);
    // Runs now, and again when `[panel.clock]` changes (the memo keeps
    // other settings' changes out).
    Effect::new(move || {
        let settings = settings.get();
        // The old clock stops before the new one starts, so its last tick
        // cannot overwrite the new clock's first.
        follow.borrow_mut().take();
        match start(env.clone(), settings, text) {
            Ok(clock) => *follow.borrow_mut() = Some(clock),
            Err(error) => report::error("clock", &error),
        }
    });
    rinch::reactive::on_cleanup(move || drop(running.borrow_mut().take()));
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

    fn write(posix: &str, settings: ClockSettings) -> Option<String> {
        let format = Formatter::new(&posix_to_unicode(posix), style(settings)).unwrap();
        text_for(tick("2026-11-10T13:05:09Z", "CET-1"), &format)
    }

    #[test]
    fn maps_the_settings_onto_the_clocks_precision_and_style() {
        let default = ClockSettings::default();
        assert_eq!(precision(default), Precision::Minutes);
        assert_eq!(style(default), Style::Time(Precision::Minutes));
        let both = ClockSettings {
            seconds: true,
            date: true,
        };
        assert_eq!(precision(both), Precision::Seconds);
        assert_eq!(
            style(both),
            Style::DateTime(Length::Medium, Precision::Seconds)
        );
    }

    #[test]
    fn shows_what_the_settings_ask_in_the_locale() {
        let minutes = ClockSettings::default();
        assert_eq!(write("de_DE.UTF-8", minutes).as_deref(), Some("14:05"));
        assert_eq!(
            write("en_US.UTF-8", minutes).as_deref(),
            Some("2:05\u{202f}PM")
        );
        let seconds = ClockSettings {
            seconds: true,
            date: false,
        };
        assert_eq!(write("de_DE.UTF-8", seconds).as_deref(), Some("14:05:09"));
        let dated = ClockSettings {
            seconds: false,
            date: true,
        };
        assert_eq!(
            write("en_GB.UTF-8", dated).as_deref(),
            Some("10 Nov 2026, 14:05")
        );
    }

    #[test]
    fn an_error_leaves_the_text_alone() {
        let format =
            Formatter::new(&posix_to_unicode("C"), style(ClockSettings::default())).unwrap();
        let error = ClockError::Watch(std::io::Error::other("no inotify"));
        assert_eq!(text_for(Event::Error(error), &format), None);
    }
}

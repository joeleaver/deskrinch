//! The clock: the time (and, if set, the date) in the user's locale,
//! changing on the minute, or on the second when it shows seconds.
//!
//! The panel has one clock, whatever the number of faces showing it:
//! [`follow`], called once at the panel's root, runs it and gives its text
//! ([`ClockText`]) to the context, and each [`ClockWidget`] is a face that
//! only reads that text.
//!
//! A [`deskrinch_clock::Clock`] wakes on its own thread when the displayed
//! time changes, the wall clock is set, the machine resumes or the time
//! zone changes, and the text is formatted there. It reaches the UI through
//! [`Signal::send`], rinch's way of writing a signal from another thread:
//! the write is dispatched to the main thread, where each face's text node
//! updates.
//!
//! The clock follows the `[panel.clock]` settings: when they change, the
//! running clock is stopped and a new one started at the new precision and
//! style. It stops with the scope [`follow`] was called in.

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

/// The clock's text, as the panel's one clock writes it: what every face
/// shows. Empty until the first tick, and while there is no clock (it could
/// not start, which is logged).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClockText(Signal<String>);

impl ClockText {
    /// The text now, read reactively.
    fn shown(self) -> String {
        self.0.get()
    }

    /// A face's text that `text` alone writes, with no clock: for the
    /// tests of what holds the faces.
    #[cfg(test)]
    pub(crate) fn fixed(text: Signal<String>) -> Self {
        Self(text)
    }
}

/// Starts the panel's one clock, following `panel`'s `[panel.clock]`, and
/// returns its text, for the context. Called once, at the panel's root; the
/// clock stops when the scope it was called in ends. Its worker writes the
/// text with `Signal::send`, so that scope is on a host thread, as a
/// hand-over's must be ([`Inbox`](deskrinch_component::Inbox)); outside a
/// scope the clock runs for the app's life.
pub(crate) fn follow(panel: Signal<PanelSettings>) -> ClockText {
    follow_running(panel).0
}

/// The clock running now, if one could start: what [`follow`] keeps.
type Running = Rc<RefCell<Option<Clock>>>;

/// [`follow`], and the clock it keeps running, for the tests to see which
/// worker it is.
fn follow_running(panel: Signal<PanelSettings>) -> (ClockText, Running) {
    let settings = Memo::new(move || panel.get().clock);
    let text = Signal::new(String::new());
    let env = Environment::from_process();
    let running: Running = Rc::default();
    let restart = Rc::clone(&running);
    // Runs now, and again when `[panel.clock]` changes (the memo keeps
    // other settings' changes out).
    Effect::new(move || {
        let settings = settings.get();
        // The old clock stops before the new one starts, so its last tick
        // cannot overwrite the new clock's first.
        restart.borrow_mut().take();
        match start(env.clone(), settings, text) {
            Ok(clock) => *restart.borrow_mut() = Some(clock),
            Err(error) => report::error("clock", &error),
        }
    });
    // Said on the main thread when the text changes, before the frame
    // that draws it: what a test reads to tell the clock's frames apart.
    Effect::new(move || tracing::debug!("{}", deskrinch_panel::clock_shows(&text.get())));
    let stop = Rc::clone(&running);
    rinch::reactive::on_cleanup(move || drop(stop.borrow_mut().take()));
    (ClockText(text), running)
}

/// A face of the clock: the context's [`ClockText`], which [`follow`]
/// writes.
#[component]
pub(crate) fn ClockWidget() -> NodeHandle {
    let text: ClockText = use_context();
    rsx! {
        span {
            class: "clock",
            style: "white-space: nowrap; font-variant-numeric: tabular-nums;",
            {move || text.shown()}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskrinch_clock::jiff::Timestamp;
    use deskrinch_clock::jiff::tz::TimeZone;
    use deskrinch_clock::{Cause, Tick, posix_to_unicode};
    use deskrinch_settings::panel::Edge;
    use deskrinch_test_wait::wait_for;
    use rinch::reactive::Scope;
    use rinch::testing::TestApp;

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

    /// The worker of the clock `running` keeps, if one runs.
    fn worker(running: &Running) -> Option<std::thread::ThreadId> {
        running.borrow().as_ref().map(Clock::worker)
    }

    /// The panel's clock is one worker: faces read its text and start none,
    /// a change of `[panel.clock]` replaces it (a new worker, the old one
    /// gone), any other change leaves it be, and it stops with the scope
    /// that started it.
    ///
    /// Counted by the clock itself (`deskrinch_clock::running`), exact at
    /// once both ways, and told apart by its worker's thread id, which Rust
    /// never gives twice; not by the threads the system lists, which show a
    /// new worker only once it has named itself and an ended one for a
    /// moment after its join. The count is the process's, so the tests that
    /// start a clock take turns ([`one_clock_at_a_time`]).
    /// Held for the whole of a test that starts a clock: the clocks running
    /// are counted for the process (`deskrinch_clock::running`), so one
    /// test's count must not see another's clock.
    fn one_clock_at_a_time() -> std::sync::MutexGuard<'static, ()> {
        static CLOCKS: std::sync::Mutex<()> = std::sync::Mutex::new(());
        CLOCKS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The clock followed in a scope of its own, made in a `TestApp`'s
    /// mount on this thread, which the app hosts: the worker's writes come
    /// here ([`Inbox`]'s rule).
    ///
    /// [`Inbox`]: deskrinch_component::Inbox
    struct Mounted {
        app: TestApp,
        /// The scope the clock was started in.
        clock: Scope,
        settings: Signal<PanelSettings>,
        text: ClockText,
        running: Running,
    }

    impl Mounted {
        fn new() -> Self {
            type Held = (Scope, Signal<PanelSettings>, ClockText, Running);
            let held: Rc<RefCell<Option<Held>>> = Rc::default();
            let out = Rc::clone(&held);
            let app = TestApp::mount(move |scope: &mut RenderScope| {
                let settings = Signal::new(PanelSettings::default());
                let clock = Scope::new();
                let (text, running) = clock.run(|| follow_running(settings));
                *out.borrow_mut() = Some((clock, settings, text, running));
                scope.create_element("div")
            });
            let (clock, settings, text, running) =
                held.borrow_mut().take().expect("made at the mount");
            Self {
                app,
                clock,
                settings,
                text,
                running,
            }
        }

        /// Ends the clock's scope, which stops the clock (its worker joined:
        /// nothing more is sent), then drains on this thread what it had
        /// sent and was still queued here. Before the app goes, so that no
        /// write of this clock waits for a thread that will end, whose host
        /// tag a later test's thread may take and drain the write in, where
        /// no such signal is. Those last writes find the text freed, which
        /// rinch says once, on this thread.
        fn stop(mut self) {
            self.clock.dispose();
            self.app.settle();
        }
    }

    #[test]
    fn one_worker_whatever_the_number_of_faces() {
        let _alone = one_clock_at_a_time();
        let mounted = Mounted::new();
        let (settings, text) = (mounted.settings, mounted.text);
        let running = Rc::clone(&mounted.running);
        assert_eq!(deskrinch_clock::running(), 1);
        let first = worker(&running);
        assert!(first.is_some());

        // Three faces, as three bars would have: each reads the one text.
        let faces: Vec<String> = (0..3).map(|_| text.shown()).collect();
        assert_eq!(faces.len(), 3);
        assert_eq!(deskrinch_clock::running(), 1);
        assert_eq!(worker(&running), first);

        let seconds = ClockSettings {
            seconds: true,
            date: false,
        };
        settings.update(|settings| settings.clock = seconds);
        assert_eq!(deskrinch_clock::running(), 1);
        let restarted = worker(&running);
        assert!(restarted.is_some());
        assert_ne!(restarted, first, "the clock was not restarted");

        settings.update(|settings| settings.edge = Edge::Top);
        assert_eq!(deskrinch_clock::running(), 1);
        assert_eq!(worker(&running), restarted);

        mounted.stop();
        assert_eq!(deskrinch_clock::running(), 0);
    }

    /// The clock's writes reach only the thread its text was made on, when
    /// that thread hosts an app. Its first tick is seen here, so the worker
    /// has written and the write came here; then a second app, on a thread
    /// of its own, settles with a capture of what it says, and says nothing.
    /// Every later tick goes here too, whenever it comes, so nothing rests
    /// on timing. (A text made on a thread that hosts nothing, as this test
    /// used to make it, has its writes go to whichever host drains first:
    /// that second app, which drops them with a warning.)
    #[test]
    fn the_clocks_writes_reach_only_its_own_thread() {
        let _alone = one_clock_at_a_time();
        let mut mounted = Mounted::new();
        let text = mounted.text;
        wait_for("the clock's first tick here", || {
            mounted.app.settle();
            if text.shown().is_empty() {
                Err("no text yet")
            } else {
                Ok(())
            }
        });
        let said = std::thread::spawn(|| {
            let log = deskrinch_test_log::capture();
            let mut other = TestApp::mount(|scope: &mut RenderScope| scope.create_element("div"));
            other.settle();
            log.warnings_and_errors()
        })
        .join()
        .expect("the other app's thread");
        assert_eq!(said, Vec::<String>::new());
        mounted.stop();
    }
}

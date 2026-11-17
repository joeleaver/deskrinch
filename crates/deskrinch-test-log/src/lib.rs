//! What a test's own thread logged through `tracing`.
//!
//! [`capture`] installs, the first time any test calls it, one subscriber
//! for the whole test process (`tracing`'s global default), and starts
//! keeping the messages of the events the calling thread emits from then
//! on, until the [`Capture`] it returns is dropped. [`Capture::lines`]
//! reads them.
//!
//! # Why one process-wide subscriber
//!
//! A test that sets a subscriber for its own thread only (`with_default`,
//! `set_default`) can lose its events: `tracing` caches each callsite's
//! interest for the whole process when the callsite is first hit, and while
//! at most one subscriber exists it asks only the subscriber of the thread
//! that hit it. A callsite first hit by another test's thread, which has no
//! subscriber of its own, is cached as uninteresting for every thread, the
//! capturing test's included. This subscriber is the global one and is
//! interested in every callsite, so no callsite is ever cached as
//! uninteresting, and each event is kept by the thread that emitted it.
//! The workspace's `clippy.toml` disallows the thread-scoped forms.
//!
//! # What is kept
//!
//! - Each event's level and `message`, written out, in order, on the
//!   thread that emitted it, while that thread captures: [`Capture::lines`]
//!   reads the messages, [`Capture::events`] both, and
//!   [`Capture::warnings_and_errors`] the messages at warn or error. An event emitted on another
//!   thread (a worker the code under test started) goes to that thread's
//!   buffer, which nothing reads unless that thread captures; it is not in
//!   the test's lines.
//! - One capture at a time on a thread: a second [`capture`] while the
//!   first is alive panics.
//! - Code under test that installs a global subscriber of its own fails to
//!   (`tracing` allows one for the life of the process, so its
//!   `set_global_default` returns an error); if it installed one before the
//!   first [`capture`], [`capture`] panics, saying so.
//!
//! # Example
//!
//! ```
//! let capture = deskrinch_test_log::capture();
//! tracing::info!("hidden after {} characters", 4);
//! assert_eq!(capture.lines(), ["hidden after 4 characters"]);
//! ```

use std::cell::RefCell;
use std::fmt::Write as _;
use std::marker::PhantomData;
use std::sync::OnceLock;

use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber, span};

thread_local! {
    /// The events kept on this thread, while it captures.
    static LINES: RefCell<Option<Vec<Logged>>> = const { RefCell::new(None) };
}

/// One event a thread logged: its level and its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logged {
    /// Its level.
    pub level: Level,
    /// Its `message` field, written out.
    pub message: String,
}

/// Captures this thread's events until dropped; not sent to another
/// thread, whose events are not its.
#[derive(Debug)]
pub struct Capture {
    /// Bound to the thread that made it.
    thread: PhantomData<*const ()>,
}

impl Capture {
    /// The messages this thread's events gave since the capture began, in
    /// order.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.events()
            .into_iter()
            .map(|event| event.message)
            .collect()
    }

    /// This thread's events since the capture began, with their levels, in
    /// order.
    #[must_use]
    pub fn events(&self) -> Vec<Logged> {
        LINES.with(|lines| lines.borrow().clone().unwrap_or_default())
    }

    /// The messages of this thread's events at warn or error level since
    /// the capture began, in order.
    #[must_use]
    pub fn warnings_and_errors(&self) -> Vec<String> {
        self.events()
            .into_iter()
            .filter(|event| event.level <= Level::WARN)
            .map(|event| event.message)
            .collect()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        LINES.with(|lines| lines.borrow_mut().take());
    }
}

/// Starts capturing this thread's events (see the crate's documentation).
///
/// # Panics
///
/// If this thread is already capturing, or another global subscriber was
/// installed before the first capture.
#[must_use = "the capture ends when it is dropped"]
pub fn capture() -> Capture {
    install();
    LINES.with(|lines| {
        let mut lines = lines.borrow_mut();
        assert!(lines.is_none(), "this thread is already capturing");
        *lines = Some(Vec::new());
    });
    Capture {
        thread: PhantomData,
    }
}

/// Installs the subscriber once for the process.
fn install() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        tracing::subscriber::set_global_default(PerThread).unwrap_or_else(|error| {
            panic!("deskrinch-test-log needs to be the global subscriber, and another is: {error}")
        });
    });
}

/// The subscriber: interested in everything, keeping each event on the
/// thread that emitted it.
struct PerThread;

impl Subscriber for PerThread {
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::always()
    }
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }
    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
    fn event(&self, event: &Event<'_>) {
        LINES.with(|lines| {
            if let Some(lines) = lines.borrow_mut().as_mut() {
                let mut message = Message::default();
                event.record(&mut message);
                lines.push(Logged {
                    level: *event.metadata().level(),
                    message: message.0,
                });
            }
        });
    }
    fn enter(&self, _: &span::Id) {}
    fn exit(&self, _: &span::Id) {}
}

/// An event's `message` field, written out.
#[derive(Default)]
struct Message(String);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            // Writing to a `String` cannot fail.
            let _written = write!(self.0, "{value:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_each_events_level_beside_its_message() {
        let capture = capture();
        tracing::info!("one");
        tracing::warn!("two");
        tracing::debug!("three");
        tracing::error!("four");
        assert_eq!(capture.lines(), ["one", "two", "three", "four"]);
        assert_eq!(
            capture.events()[1],
            Logged {
                level: Level::WARN,
                message: "two".to_owned()
            }
        );
        assert_eq!(capture.warnings_and_errors(), ["two", "four"]);
    }
}

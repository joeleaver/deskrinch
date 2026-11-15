//! The manual clock moved on, and the service's expiry pass for the new
//! time known to have run: what the tests that check that nothing expires
//! wait for instead of watching a quiet window.

use std::time::Duration;

use deskrinch_notification_service::{Handle, ManualClock};

use crate::common::{Client, Events, Signal, id};

/// A client's clock, with what the sentinels need.
pub(crate) struct Time<'a> {
    pub client: &'a Client,
    pub clock: &'a ManualClock,
    pub handle: Handle,
    pub events: &'a Events,
}

impl Time<'_> {
    /// Advances the clock by `by` and returns once the service has expired
    /// what is due by then, panicking if the client got any signal
    /// meanwhile.
    ///
    /// Two sentinels of the client's own make it sound. The worker records
    /// expiries only in its pass over the live notifications, whose order it
    /// does not state, and delivers each pass as one batch:
    ///
    /// - the first, made just before the advance with a timeout of `by`,
    ///   expires in the pass for the new time: its close coming first shows
    ///   that the pass ran, and nothing before it;
    /// - the second, made and dismissed after, is recorded after that pass:
    ///   its close coming next shows nothing else was in the first's batch.
    ///
    /// Their events are left out of [`Events`].
    #[track_caller]
    pub(crate) fn advance_quietly(&self, by: Duration) {
        let timeout = i32::try_from(by.as_millis()).expect("an advance within i32 milliseconds");
        assert!(timeout > 0, "an advance of at least a millisecond");
        let first = self.client.simple(0, "sentinel", "", timeout);
        self.events.leave_out(first);
        self.clock.advance(by);
        let signals = &self.client.signals;
        assert_eq!(
            signals.next(),
            Signal::Closed(first, 1),
            "expected only the sentinel's expiry"
        );
        let second = self.client.simple(0, "sentinel", "", 0);
        self.events.leave_out(second);
        self.handle.dismiss(id(second)).unwrap();
        assert_eq!(
            signals.next(),
            Signal::Closed(second, 2),
            "expected only the sentinel's dismissal"
        );
    }
}

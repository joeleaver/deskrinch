//! When an item is read: one read at a time, at most one more asked for
//! while it runs, and never two starts closer than the read interval.

use std::time::{Duration, Instant};

use crate::item::ItemId;

/// A read to start: of item `id`, at object `path` of connection `owner`
/// (a unique name, which the bus never activates), no earlier than `at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Due {
    pub(crate) id: ItemId,
    pub(crate) owner: String,
    pub(crate) path: String,
    pub(crate) at: Instant,
}

/// One item's reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Reads {
    state: State,
    /// When the last read started, or is to start.
    last_start: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// No read is due or in flight.
    Idle,
    /// A read is due or in flight.
    Busy,
    /// A read is due or in flight, and one more is asked for after it.
    BusyThenAgain,
}

impl Reads {
    /// A newly registered item's reads, with its first read due at `now`.
    pub(super) fn first(now: Instant) -> Self {
        Self {
            state: State::Busy,
            last_start: Some(now),
        }
    }

    /// Something changed at `now`: when to start a read, unless one is due
    /// or in flight already. A read still waiting for its start will see
    /// the change; after one in flight, one more is asked for.
    pub(super) fn ask(&mut self, now: Instant, interval: Duration) -> Option<Instant> {
        match self.state {
            State::Idle => {
                self.state = State::Busy;
                Some(self.schedule(now, interval))
            }
            State::Busy if self.last_start.is_some_and(|start| now < start) => None,
            State::Busy | State::BusyThenAgain => {
                self.state = State::BusyThenAgain;
                None
            }
        }
    }

    /// The read in flight ended at `now`: when to start the one asked for
    /// after it, if any.
    pub(super) fn done(&mut self, now: Instant, interval: Duration) -> Option<Instant> {
        match self.state {
            State::BusyThenAgain => {
                self.state = State::Busy;
                Some(self.schedule(now, interval))
            }
            State::Idle | State::Busy => {
                self.state = State::Idle;
                None
            }
        }
    }

    /// The next start: no sooner than `now`, nor than `interval` after the
    /// last start.
    fn schedule(&mut self, now: Instant, interval: Duration) -> Instant {
        let at = self
            .last_start
            .map_or(now, |last| (last + interval).max(now));
        self.last_start = Some(at);
        at
    }
}

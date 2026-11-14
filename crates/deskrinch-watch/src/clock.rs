//! Where the watching thread takes the time from: the system's monotonic
//! clock, or, in the crate's tests, one the test moves, so that a test
//! decides exactly what time the debounce sees.

use std::time::Instant;

/// The time, as the worker reads it.
pub(crate) trait Clock: Send {
    /// Now.
    fn now(&self) -> Instant;
}

/// The system's monotonic clock.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct System;

impl Clock for System {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

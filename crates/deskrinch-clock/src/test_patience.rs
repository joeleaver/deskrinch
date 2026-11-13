//! How long the clock's tests wait for something before they call it
//! hung: no bound on how late the machine runs a woken thread, only a
//! guard against a broken clock hanging the run. It is to be
//! `deskrinch_test_wait::PATIENCE` once that crate exists. The timer's
//! unit tests use it as a module; the real-clock tests (`tests/clock.rs`)
//! include this file.

use std::time::Duration;

/// How long a test waits before it calls the clock hung.
pub(crate) const PATIENCE: Duration = Duration::from_secs(60);

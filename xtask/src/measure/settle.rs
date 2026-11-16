//! When a summoned program counts as settled: the waits `--summon`
//! measures between ([`super::show`]).
//!
//! Settled means a window in which its threads ran almost nothing and rinch
//! prepared no frame for its hidden window (`[PERF] prepared N:`), so a show
//! is never summoned while a preparation goes on.
//!
//! A quiet window alone does not say the program has started. A program
//! starved of CPU before its UI's first turn passes one having done
//! nothing, and its first summon would then pay for its start. A program
//! that prepares its hidden window (`--prepares`) is therefore waited for
//! until it has prepared once before its quiet window counts. Measure
//! cannot ask that of every program: it does not know whether one prepares.

use std::process::Child;
use std::time::{Duration, Instant};

use deskrinch_test_component::Watch;

use super::probe::nanos;
use super::procfs::{self, cpu_between};
use crate::error::Error;
use crate::session::interrupt;

/// A program counts as settled when its threads ran less than this much
/// CPU time over [`SETTLE_WINDOW`].
const SETTLED_CPU: Duration = Duration::from_millis(5);
/// The window [`SETTLED_CPU`] is measured over.
const SETTLE_WINDOW: Duration = Duration::from_secs(1);
/// How often a wait looks again.
const POLL: Duration = Duration::from_millis(100);

/// Waits for the program, just started, to settle, hidden: with `prepares`,
/// first for its first prepared frame (within `timeout`). A frame before
/// the first summon means it did not start hidden.
pub(super) fn settle_hidden(
    child: &mut Child,
    watch: &Watch,
    program: &str,
    timeout: Duration,
    prepares: bool,
) -> Result<(), Error> {
    if prepares {
        first_preparation(child, watch, program, timeout)?;
    }
    settle(child, watch, program, timeout)?;
    shown_before_summon(watch, program)
}

/// Waits, within `timeout`, for the program's first prepared frame.
fn first_preparation(
    child: &mut Child,
    watch: &Watch,
    program: &str,
    timeout: Duration,
) -> Result<(), Error> {
    let start = Instant::now();
    while !watch.wait_prepared_after(0, POLL) {
        interrupt::check()?;
        alive(child, watch, program)?;
        shown_before_summon(watch, program)?;
        if start.elapsed() >= timeout {
            return Err(Error::NeverPrepared {
                program: program.to_owned(),
                waited: timeout,
                output: watch.tail(),
            });
        }
    }
    Ok(())
}

/// Fails if the program has presented a frame: it did not start hidden.
fn shown_before_summon(watch: &Watch, program: &str) -> Result<(), Error> {
    if watch.frames() > 0 {
        return Err(Error::ShownBeforeSummon {
            program: program.to_owned(),
            output: watch.tail(),
        });
    }
    Ok(())
}

/// Waits until the program's threads run less than [`SETTLED_CPU`] over
/// [`SETTLE_WINDOW`], with no prepared frame in it.
pub(super) fn settle(
    child: &mut Child,
    watch: &Watch,
    program: &str,
    timeout: Duration,
) -> Result<(), Error> {
    let pid = child.id();
    let start = Instant::now();
    let mut before = procfs::read_threads(pid);
    let mut prepared = watch.prepared();
    loop {
        let window_end = Instant::now() + SETTLE_WINDOW;
        while Instant::now() < window_end {
            interrupt::check()?;
            alive(child, watch, program)?;
            std::thread::sleep(POLL);
        }
        let now = procfs::read_threads(pid);
        let prepared_now = watch.prepared();
        if settled(cpu_between(&before, &now), prepared, prepared_now) {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            return Err(Error::NeverSettled {
                program: program.to_owned(),
                waited: timeout,
                output: watch.tail(),
            });
        }
        before = now;
        prepared = prepared_now;
    }
}

/// Whether a settle window was quiet: its threads ran less than
/// [`SETTLED_CPU`] (`cpu_ns`), and no prepared frame came in it (the main
/// window's prepared frames went from `prepared_before` to
/// `prepared_after`). A preparation queued but not yet run shows in
/// neither until it runs; one that ran shows in the second however little
/// CPU it took, so a show is never summoned while a preparation goes on.
fn settled(cpu_ns: u64, prepared_before: u64, prepared_after: u64) -> bool {
    cpu_ns < nanos(SETTLED_CPU) && prepared_after == prepared_before
}

/// Fails if the program has exited.
pub(super) fn alive(child: &mut Child, watch: &Watch, program: &str) -> Result<(), Error> {
    match child.try_wait() {
        Ok(Some(status)) => Err(Error::MeasuredExited {
            program: program.to_owned(),
            status,
            output: watch.last_words(),
        }),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settled_wants_quiet_threads_and_no_prepared_frame() {
        let quiet = nanos(SETTLED_CPU) - 1;
        assert!(settled(quiet, 2, 2));
        assert!(!settled(quiet, 2, 3), "a preparation ran in the window");
        assert!(!settled(nanos(SETTLED_CPU), 2, 2), "the threads ran");
    }
}

//! The waits `--summon` and the steps measure between: each until the
//! program is quiet ([`super::quiet`]), so a show is never summoned while
//! a preparation goes on, nor a step taken while the last one's work, or
//! its compositor's, is still running or waiting to run.
//!
//! A quiet window alone does not say a program that prepares its hidden
//! window has prepared it: one that has not yet been given the CPU is
//! runnable, and so not quiet, but one waiting on something else (a file,
//! a socket) before its first turn could be. A program that prepares
//! (`--prepares`) is therefore waited for until it has prepared once.

use std::process::Child;
use std::time::{Duration, Instant};

use deskrinch_test_component::Watch;

use super::probe::nanos;
use super::quiet::{self, Neighbours, Quieted, Watched};
use super::report::{Run, Unit};
use crate::error::Error;
use crate::session::interrupt;

/// The measured program while it runs: what waiting on it needs.
pub(crate) struct Running<'a> {
    pub(crate) child: &'a mut Child,
    pub(crate) watch: &'a Watch,
    pub(crate) program: &'a str,
    /// How long whatever it is waited for may take.
    pub(crate) timeout: Duration,
    /// The session's processes it may wait on.
    pub(crate) neighbours: Neighbours,
}

impl Running<'_> {
    /// Waits for it to be quiet; what the wait took.
    pub(crate) fn settle(&mut self) -> Result<Quieted, Error> {
        settle(
            self.child,
            self.watch,
            self.program,
            self.timeout,
            self.neighbours,
        )
    }

    /// Fails if it has exited.
    pub(crate) fn alive(&mut self) -> Result<(), Error> {
        alive(self.child, self.watch, self.program)
    }
}

/// How often a wait for a preparation looks again.
const POLL: Duration = Duration::from_millis(100);

/// Waits for the program, just started, to be quiet, hidden: with
/// `prepares`, first for its first prepared frame (within `timeout`). A
/// frame before the first summon means it did not start hidden.
pub(super) fn settle_hidden(running: &mut Running<'_>, prepares: bool) -> Result<Quieted, Error> {
    if prepares {
        first_preparation(
            running.child,
            running.watch,
            running.program,
            running.timeout,
        )?;
    }
    let quieted = running.settle()?;
    shown_before_summon(running.watch, running.program)?;
    Ok(quieted)
}

/// Records a wait that ended quiet as the row `{what}: wall time of the
/// settle, to quiet`, and notes any window of it thrown out because the
/// tool's own polls came late.
pub(crate) fn record(run: &mut Run, what: &str, quieted: Quieted) {
    run.push(
        format!("{what}: wall time of the settle, to quiet"),
        Unit::WallNs,
        nanos(quieted.took),
    );
    if quieted.late_windows > 0 {
        run.note(
            format!("{what}: settle windows thrown out because the tool's own polls came late"),
            quieted.late_windows.to_string(),
        );
    }
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

/// Waits, within `timeout`, until the program and its neighbours are quiet
/// ([`quiet::wait`]); fails, saying what was not quiet, if they are not.
pub(super) fn settle(
    child: &mut Child,
    watch: &Watch,
    program: &str,
    timeout: Duration,
    neighbours: Neighbours,
) -> Result<Quieted, Error> {
    let watched = Watched {
        program: child.id(),
        neighbours,
    };
    match quiet::wait(&watched, child, watch, program, timeout)? {
        Ok(quieted) => Ok(quieted),
        Err(unquiet) => Err(Error::NeverQuiet {
            program: program.to_owned(),
            waited: unquiet.waited,
            said: unquiet.said(),
            output: watch.tail(),
        }),
    }
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

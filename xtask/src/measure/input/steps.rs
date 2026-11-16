//! Steps measured one at a time: each taken alone, followed by a frame, and
//! then the settle, measured from just before it to settled ([`Meter`]).
//! The keys of `--type` and the pointer moves of `--move-pointer` are both
//! measured here.
//!
//! A step's span can hold more than the step: the program's own work that
//! falls in it, such as a clock's minute. So each step's frames are
//! counted with how many ran reactive work (a signal notified, rinch's
//! `signal_notifies` in the frame's line). A reactive frame is no anomaly
//! in itself (typing changes a query); a step that ran otherwise in one run
//! than in the others is, and the report notes it where the runs are seen
//! together ([`super::super::report::note_odd_steps`]).
//!
//! The frames are read once the meter has stopped (under Callgrind, once
//! the step's dump is written), so they cover the span the figure does: a
//! frame between the settle and the dump is in both. A frame's line is
//! written when it is presented, after its work, so a frame whose work came
//! just before the step began is counted in it too.

use std::time::{Duration, Instant};

use deskrinch_test_component::{PerfLine, PerfWindow, Watch, counter, perf};

use super::super::report::{Run, Unit};
use super::super::settle::Running;
use super::{InputClient, MeasuredInputError, Meter};
use crate::error::Error;
use crate::input::Step;
use crate::session::interrupt;

/// One step to measure.
pub(crate) struct Measured {
    /// What the client is given.
    pub(crate) step: Step,
    /// Its row's name, after the rows' prefix: `key 1 of 4`.
    pub(crate) row: String,
    /// What a failure names it as: `key 1 of 4 ('t')`.
    pub(crate) named: String,
    /// The name of its Callgrind dump, where the meter keeps one.
    pub(crate) dump: String,
}

/// How a run of steps is reported.
pub(crate) struct Rows<'a> {
    /// What every row starts with: `show 1: `, or nothing.
    pub(crate) prefix: &'a str,
    /// What a failure adds after the step's name: ` in show 1`, or nothing.
    pub(crate) during: &'a str,
    /// What the measure starts from in the rows' words: `key`, `move`.
    pub(crate) from: &'a str,
    /// The row of their sum, after the prefix.
    pub(crate) sum: &'a str,
    /// The row of the frames they led to, after the prefix.
    pub(crate) frames: &'a str,
}

/// Takes each of `steps` through `client`, each followed by a frame within
/// the running program's timeout and then the settle, and records what
/// `meter` measured of each and of them all, and the frames.
pub(crate) fn measure_steps(
    run: &mut Run,
    running: &mut Running<'_>,
    client: &mut InputClient,
    steps: &[Measured],
    rows: &Rows<'_>,
    meter: &mut dyn Meter,
) -> Result<(), Error> {
    let frames_before = running.watch.frames();
    let mut total = 0;
    for measured in steps {
        let frames = running.watch.frames();
        let mark = running.watch.lines().len();
        meter.start()?;
        client.step(&measured.step)?;
        if !frame_after(running, frames)? {
            return Err(MeasuredInputError::NoFrame {
                program: running.program.to_owned(),
                step: format!("{}{}", measured.named, rows.during),
                waited: running.timeout,
                output: running.watch.tail(),
            }
            .into());
        }
        running.settle()?;
        let (cost, seen) = end_step(meter, running.watch, mark, &measured.dump)?;
        let row = format!("{}{}", rows.prefix, measured.row);
        run.push(
            format!("{row}: {}", meter.what(rows.from)),
            meter.unit(),
            cost,
        );
        run.push(
            format!("{row}: frames presented"),
            Unit::Count,
            seen.presented,
        );
        run.push(
            format!("{row}: of them with reactive work"),
            Unit::Count,
            seen.reactive,
        );
        total += cost;
    }
    run.push(
        format!("{}{}: {}", rows.prefix, rows.sum, meter.what(rows.from)),
        meter.unit(),
        total,
    );
    run.push(
        format!("{}{}", rows.prefix, rows.frames),
        Unit::Count,
        running.watch.frames() - frames_before,
    );
    Ok(())
}

/// The main window's frames in a step's span, and how many of them ran
/// reactive work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Seen {
    presented: u64,
    reactive: u64,
}

impl Seen {
    /// Counts the frames among `lines`.
    fn of(lines: &[String]) -> Self {
        lines
            .iter()
            .filter(|line| {
                matches!(
                    perf::parse(line),
                    Some(PerfLine::Frame {
                        window: PerfWindow::Main,
                        ..
                    })
                )
            })
            .fold(Self::default(), |seen, line| Self {
                presented: seen.presented + 1,
                reactive: seen.reactive + u64::from(reactive(line)),
            })
    }
}

/// Whether the frame line `line` ran reactive work: a signal notified.
fn reactive(line: &str) -> bool {
    counter(line, "signal_notifies").is_some_and(|n| n > 0)
}

/// Stops `meter` (writing the step's dump, `dump`, where it keeps one), and
/// only then counts the frames written since `mark`: they cover the span
/// the figure does.
fn end_step(
    meter: &mut dyn Meter,
    watch: &Watch,
    mark: usize,
    dump: &str,
) -> Result<(u64, Seen), Error> {
    let cost = meter.stop(dump)?;
    let lines = watch.lines();
    Ok((cost, Seen::of(lines.get(mark..).unwrap_or_default())))
}

/// Waits, within the running program's timeout, for a frame after the
/// first `frames`; whether one came.
fn frame_after(running: &mut Running<'_>, frames: u64) -> Result<bool, Error> {
    let start = Instant::now();
    while !running
        .watch
        .wait_frame_after(frames, Duration::from_millis(100))
    {
        interrupt::check()?;
        running.alive()?;
        if start.elapsed() >= running.timeout {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;

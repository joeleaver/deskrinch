//! Steps measured one at a time: each taken alone, followed by a frame, and
//! then the settle, measured from just before it to settled ([`Meter`]).
//! The keys of `--type` and the pointer moves of `--move-pointer` are both
//! measured here.

use std::time::{Duration, Instant};

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
        let cost = meter.stop(&measured.dump)?;
        run.push(
            format!("{}{}: {}", rows.prefix, measured.row, meter.what(rows.from)),
            meter.unit(),
            cost,
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

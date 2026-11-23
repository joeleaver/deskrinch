//! Steps measured one at a time: each taken alone, followed by a frame (or,
//! for a command step, any change the program shows), and then the settle,
//! measured from just before it to settled ([`Meter`]). The keys of
//! `--type`, the pointer moves of `--move-pointer` and the commands of
//! `--step-command` are all measured here.
//!
//! A key or a pointer move is counted in the main window's frames and must
//! lead to one. A command step is another program's command, whose effect
//! may be on any window, or may open or close one (a notification shown,
//! then closed): its frames are every window's, the windows it opened and
//! closed are noted, and it must lead to a frame on some window or to a
//! window opened or closed.
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
use super::{MeasuredInputError, Meter};
use crate::error::Error;
use crate::session::interrupt;

/// One step to measure, its action an `A` (a key or a move the input client
/// is given, a command run).
pub(crate) struct Measured<A> {
    /// What the step does.
    pub(crate) action: A,
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
    /// Whose frames are counted, and what a step must lead to.
    pub(crate) scope: Scope,
}

/// Whose frames a run of steps counts, and what each step must lead to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The main window's frames; each step leads to one of them (a key, a
    /// pointer move).
    Main,
    /// Every window's frames; each step leads to a frame on any window or
    /// to a window opened or closed, which are noted (a command step).
    Every,
}

/// Takes each of `steps` by `act`, each followed by what `rows.scope` asks
/// within the running program's timeout and then the settle, and records
/// what `meter` measured of each and of them all, and the frames.
pub(crate) fn measure_steps<A>(
    run: &mut Run,
    running: &mut Running<'_>,
    act: &mut dyn FnMut(&A) -> Result<(), Error>,
    steps: &[Measured<A>],
    rows: &Rows<'_>,
    meter: &mut dyn Meter,
) -> Result<(), Error> {
    let first = running.watch.lines().len();
    let mut total = 0;
    for measured in steps {
        let frames = running.watch.frames();
        let mark = running.watch.lines().len();
        meter.start()?;
        act(&measured.action)?;
        if !changed(running, rows.scope, frames, mark)? {
            let (program, step) = (
                running.program.to_owned(),
                format!("{}{}", measured.named, rows.during),
            );
            let (waited, output) = (running.timeout, running.watch.tail());
            return Err(match rows.scope {
                Scope::Main => MeasuredInputError::NoFrame {
                    program,
                    step,
                    waited,
                    output,
                },
                Scope::Every => MeasuredInputError::NoChange {
                    program,
                    step,
                    waited,
                    output,
                },
            }
            .into());
        }
        let settled = running.settle()?;
        let (cost, seen) = end_step(meter, running.watch, mark, &measured.dump, rows.scope)?;
        let row = format!("{}{}", rows.prefix, measured.row);
        super::super::settle::record(run, &row, settled);
        if rows.scope == Scope::Every {
            run.note(format!("{row}: windows"), seen.windows());
        }
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
    let lines = running.watch.lines();
    run.push(
        format!("{}{}", rows.prefix, rows.frames),
        Unit::Count,
        Seen::of(lines.get(first..).unwrap_or_default(), rows.scope).presented,
    );
    Ok(())
}

/// The frames in a step's span (the main window's, or every window's), how
/// many of them ran reactive work, and the windows opened and closed in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Seen {
    presented: u64,
    reactive: u64,
    opened: Vec<String>,
    closed: Vec<u64>,
}

impl Seen {
    /// Counts the frames among `lines` that `scope` counts, and notes the
    /// windows opened and closed.
    fn of(lines: &[String], scope: Scope) -> Self {
        let mut seen = Self::default();
        for line in lines {
            match perf::parse(line) {
                Some(PerfLine::Frame { window, .. })
                    if scope == Scope::Every || window == PerfWindow::Main =>
                {
                    seen.presented += 1;
                    seen.reactive += u64::from(reactive(line));
                }
                Some(PerfLine::Opened(opened)) => seen.opened.push(match &opened.output {
                    Some(output) => {
                        format!("window {} ({:?} on {output})", opened.id, opened.title)
                    }
                    None => format!("window {} ({:?})", opened.id, opened.title),
                }),
                Some(PerfLine::Closed { id }) => seen.closed.push(id),
                _ => {}
            }
        }
        seen
    }

    /// The windows opened and closed, for a run's notes.
    fn windows(&self) -> String {
        let opened = self.opened.iter().map(|window| format!("opened {window}"));
        let closed = self.closed.iter().map(|id| format!("closed window {id}"));
        let all: Vec<String> = opened.chain(closed).collect();
        if all.is_empty() {
            "none opened or closed".to_owned()
        } else {
            all.join("; ")
        }
    }

    /// Whether a step's span shows a change: a frame counted, or a window
    /// opened or closed.
    fn shows_a_change(&self) -> bool {
        self.presented > 0 || !self.opened.is_empty() || !self.closed.is_empty()
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
    scope: Scope,
) -> Result<(u64, Seen), Error> {
    let cost = meter.stop(dump)?;
    let lines = watch.lines();
    Ok((cost, Seen::of(lines.get(mark..).unwrap_or_default(), scope)))
}

/// Waits, within the running program's timeout, for what `scope` asks of a
/// step begun with `frames` main-window frames and `mark` lines of output:
/// a main-window frame after them, or a change on any window since `mark`;
/// whether it came.
fn changed(
    running: &mut Running<'_>,
    scope: Scope,
    frames: u64,
    mark: usize,
) -> Result<bool, Error> {
    let start = Instant::now();
    loop {
        let came = match scope {
            Scope::Main => running
                .watch
                .wait_frame_after(frames, Duration::from_millis(100)),
            Scope::Every => {
                let lines = running.watch.lines();
                let shows = Seen::of(lines.get(mark..).unwrap_or_default(), scope).shows_a_change();
                if !shows {
                    std::thread::sleep(Duration::from_millis(100));
                }
                shows
            }
        };
        if came {
            return Ok(true);
        }
        interrupt::check()?;
        running.alive()?;
        if start.elapsed() >= running.timeout {
            return Ok(false);
        }
    }
}

#[cfg(test)]
mod tests;

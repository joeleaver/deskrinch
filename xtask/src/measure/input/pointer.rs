//! `--place-pointer` and `--move-pointer`: the pointer placed, unmeasured,
//! then moved a step at a time, each step measured ([`super::steps`]).
//!
//! The pointer's first position is not known, so one placing comes first:
//! the client moves it there and the program is left to settle, with no
//! frame asked for. Then each move must lead to a frame, as a key does: a
//! step that changes nothing is not a measurement, and on a loaded machine
//! a quiet second alone cannot tell "no frame" from "not yet handled".
//!
//! A point is on one of the session's outputs, named by its number in the
//! order the session made them (`X,Y` on the first, `N:X,Y` on the Nth), in
//! that output's logical pixels: output names differ by compositor, and the
//! caller need not know them. Measure resolves the number to the name it
//! gives the client ([`Pointer::steps`]).

use std::fmt;

use super::super::report::Run;
use super::super::settle::Running;
use super::steps::{Measured, Rows, Scope, measure_steps};
use super::{InputClient, MeasuredInputError, Meter};
use crate::error::Error;
use crate::input::Step;

/// A point on one of the session's outputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Point {
    /// The output's number, from 1, in the order the session made them.
    output: u32,
    /// Logical pixels from its left edge.
    x: f64,
    /// Logical pixels from its top edge.
    y: f64,
}

impl Point {
    /// `X,Y` (on the first output) or `N:X,Y` (on the Nth).
    ///
    /// # Errors
    ///
    /// A message saying what a point is written as.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let wrong = || format!("{text:?} is not X,Y or N:X,Y (N an output's number from 1)");
        let (output, at) = match text.split_once(':') {
            Some((n, at)) => (
                n.parse::<u32>()
                    .ok()
                    .filter(|&n| n >= 1)
                    .ok_or_else(wrong)?,
                at,
            ),
            None => (1, text),
        };
        let (x, y) = at.split_once(',').ok_or_else(wrong)?;
        let coordinate = |word: &str| {
            word.parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && *n >= 0.0)
        };
        match (coordinate(x), coordinate(y)) {
            (Some(x), Some(y)) => Ok(Self { output, x, y }),
            _ => Err(wrong()),
        }
    }
}

/// As [`Point::parse`] reads it, its output's number always written.
impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{},{}", self.output, self.x, self.y)
    }
}

/// What the pointer options ask for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pointer {
    /// `--place-pointer`: where the pointer is put first, unmeasured.
    pub(crate) place: Point,
    /// `--move-pointer`: each measured step, in order.
    pub(crate) moves: Vec<Point>,
}

impl Pointer {
    /// Fails, before any session starts, if a point is on an output past
    /// the session's `outputs`.
    pub(crate) fn check(&self, outputs: u32) -> Result<(), Error> {
        let past = std::iter::once(&self.place)
            .chain(&self.moves)
            .find(|point| point.output > outputs);
        match past {
            Some(point) => Err(MeasuredInputError::NoSuchOutput {
                point: point.to_string(),
                outputs,
            }
            .into()),
            None => Ok(()),
        }
    }

    /// The client's steps, each point's output named by `names`, the
    /// session's outputs in the order it made them.
    pub(crate) fn steps(&self, names: &[String]) -> Result<PointerSteps, Error> {
        let step = |point: &Point| {
            let index = usize::try_from(point.output - 1).unwrap_or(usize::MAX);
            match names.get(index) {
                Some(name) => Ok(Step::Point {
                    output: name.clone(),
                    x: point.x,
                    y: point.y,
                }),
                None => Err(MeasuredInputError::NoSuchOutput {
                    point: point.to_string(),
                    outputs: u32::try_from(names.len()).unwrap_or(u32::MAX),
                }),
            }
        };
        Ok(PointerSteps {
            place: step(&self.place)?,
            moves: self
                .moves
                .iter()
                .map(|point| Ok((step(point)?, *point)))
                .collect::<Result<_, MeasuredInputError>>()?,
        })
    }

    /// What the report's header says of it.
    pub(crate) fn describe(&self) -> String {
        format!(
            "placed at {}, then {} measured step(s)",
            self.place,
            self.moves.len()
        )
    }
}

/// The pointer's steps, their outputs named.
#[derive(Debug, Clone)]
pub(crate) struct PointerSteps {
    place: Step,
    moves: Vec<(Step, Point)>,
}

/// Places the pointer and lets the program settle, then takes each move,
/// measured, its rows starting with `prefix` (`show 1: `, or nothing) and
/// its dumps' names with `dumps` (`show-1-`, or nothing).
pub(crate) fn move_pointer(
    run: &mut Run,
    running: &mut Running<'_>,
    client: &mut InputClient,
    steps: &PointerSteps,
    meter: &mut dyn Meter,
    prefix: &str,
    dumps: &str,
) -> Result<(), Error> {
    client.step(&steps.place)?;
    let settled = running.settle()?;
    super::super::settle::record(run, &format!("{prefix}the pointer placed"), settled);
    let count = steps.moves.len();
    let measured: Vec<Measured<Step>> = steps
        .moves
        .iter()
        .enumerate()
        .map(|(index, (step, point))| {
            let row = format!("pointer step {} of {count} (to {point})", index + 1);
            Measured {
                action: step.clone(),
                named: row.clone(),
                row,
                dump: format!("{dumps}pointer-{}", index + 1),
            }
        })
        .collect();
    let during = prefix
        .strip_suffix(": ")
        .map_or_else(String::new, |show| format!(" in {show}"));
    let rows = Rows {
        prefix,
        during: &during,
        from: "move",
        sum: "pointer, the steps' sum",
        frames: "pointer: frames presented",
        scope: Scope::Main,
    };
    measure_steps(
        run,
        running,
        &mut |step| client.step(step),
        &measured,
        &rows,
        meter,
    )
}

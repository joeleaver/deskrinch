//! Input measure gives the program: a query typed (`--type`, [`typing`])
//! and the pointer moved (`--place-pointer`, `--move-pointer`,
//! [`pointer`](mod@pointer)), each step measured on its own ([`steps`]).
//!
//! A figure that depended on how fast steps arrive would not be a
//! measurement: under Callgrind the program is far slower than any input
//! device, and steps would pile into one frame. So each step is taken
//! alone: the step, a frame after it, then the program left to settle (the
//! settle `--summon` waits for), then the next. Each is measured from just
//! before it to settled ([`Meter`]).
//!
//! The steps come from [`InputClient`], `cargo xtask input` run in the
//! session ([`crate::input`] has its protocol). It is started before the
//! program: a headless wlroots seat gives a window that maps while it has
//! no keyboard none of the keyboard, and devices made later would land in
//! the program's start-up and idle figures.

mod client;
mod command;
mod meter;
mod pointer;
mod setup;
mod steps;
mod typing;

pub(crate) use client::InputClient;
pub(crate) use command::{Commands, run_commands};
pub(crate) use meter::{CallgrindMeter, CpuMeter, Meter};
pub(crate) use pointer::{Point, Pointer, PointerSteps, move_pointer};
pub(crate) use setup::run_setup;
pub(crate) use typing::{QueryLine, Typing, after_show, check_query_line, parse_text};

use std::process::Command;
use std::time::Duration;

use crate::error::Error;

/// What a run gives the program through the input client, and how to start
/// the client.
pub(crate) struct Inputs<'a> {
    /// `cargo xtask input`, as a client of the session.
    pub(crate) client: &'a dyn Fn() -> Command,
    /// With the pointer options: the pointer's steps, outputs named.
    pub(crate) pointer: Option<&'a PointerSteps>,
}

impl Inputs<'_> {
    /// Starts the client.
    pub(crate) fn start(&self) -> Result<InputClient, Error> {
        InputClient::start((self.client)())
    }
}

/// Why input could not be measured.
#[derive(Debug, thiserror::Error)]
pub(crate) enum MeasuredInputError {
    /// The program never said a key would reach where it types.
    #[error(
        "{program} never wrote a line with {line:?} after show {show} (waited {waited:?}): a \
         program that cannot say a key typed reaches it cannot be measured typed\nIts last \
         output:\n{output}"
    )]
    NoKeyboardLine {
        program: String,
        show: u32,
        line: String,
        waited: Duration,
        output: String,
    },
    /// A step led to no frame.
    #[error(
        "{program} presented no frame for {step} (waited {waited:?})\nIts last output:\n{output}"
    )]
    NoFrame {
        program: String,
        step: String,
        waited: Duration,
        output: String,
    },
    /// A command step led to no frame on any window and opened or closed
    /// no window.
    #[error(
        "{program} showed no change for {step}: no frame on any window, no window opened or \
         closed (waited {waited:?})\nIts last output:\n{output}"
    )]
    NoChange {
        program: String,
        step: String,
        waited: Duration,
        output: String,
    },
    /// A command (a step's, or a setup command) ended with a status other
    /// than 0. `what` names it: `the step's command`, `setup command 1 of 2`.
    #[error("{what} {command:?} ended {status}; it said:\n{output}")]
    CommandFailed {
        what: String,
        command: String,
        status: std::process::ExitStatus,
        output: String,
    },
    /// A command did not end within the first-frame timeout.
    #[error("{what} {command:?} did not end within {waited:?}, and was ended")]
    CommandHung {
        what: String,
        command: String,
        waited: Duration,
    },
    /// The program wrote no line giving its query's length at the hide.
    #[error(
        "{program} wrote no line like {pattern:?} at hide {show} (waited {waited:?})\nIts last \
         output:\n{output}"
    )]
    NoQueryLine {
        program: String,
        show: u32,
        pattern: String,
        waited: Duration,
        output: String,
    },
    /// The query had another length than the text typed: a key was lost.
    #[error(
        "{program} said at hide {show} its query had {said} characters, but {typed} were typed: \
         a key was lost, and the figures would be a shorter query's\nIts last output:\n{output}"
    )]
    WrongLength {
        program: String,
        show: u32,
        typed: usize,
        said: usize,
        output: String,
    },
    /// A point is on an output the session does not have.
    #[error("the point {point} is on an output past the session's {outputs} (--outputs)")]
    NoSuchOutput { point: String, outputs: u32 },
    /// The input client could not be run, refused a step, or stopped.
    #[error("the input client (cargo xtask input) {what}{}", stderr_of(.stderr))]
    Client { what: String, stderr: String },
}

fn stderr_of(stderr: &str) -> String {
    if stderr.trim().is_empty() {
        String::new()
    } else {
        format!("; it said:\n{}", stderr.trim_end())
    }
}

#[cfg(test)]
mod tests;

//! `--type`: what it costs a shown program to take a query typed into it,
//! one key at a time.
//!
//! A figure that depended on how fast keys arrive would not be a
//! measurement: under Callgrind the program is far slower than any typist,
//! and keys would pile into one frame. So each key is typed alone: the key,
//! then the program left to settle (the settle `--summon` waits for), then
//! the next. Each key is measured from just before it is typed to settled
//! ([`Meter`]), and must lead to a frame.
//!
//! The keys come from [`Typist`], `cargo xtask type` run in the session.
//! What measure cannot see from outside, the program says in its output,
//! in words `--type` is given and holds none of itself
//! ([`Typing::keyboard_line`], [`QueryLine`]): that a key typed now reaches
//! where it types, which the first key waits for, and at the hide how many
//! characters it was typed, which must be the text's. A key lost would
//! otherwise make the figure that of a shorter query, without saying so.

mod meter;
mod typist;

pub(crate) use meter::{CallgrindMeter, CpuMeter, Meter};
pub(crate) use typist::Typist;

use std::time::{Duration, Instant};

use deskrinch_test_input::Key;

use super::report::{Run, Unit};
use super::show::Running;
use crate::error::Error;
use crate::session::interrupt;

/// What `--type` asks for.
#[derive(Debug, Clone)]
pub(crate) struct Typing {
    /// What is typed after each show.
    pub(crate) text: String,
    /// Words of the line the program writes once a key typed reaches where
    /// it types (`--keyboard-line`).
    pub(crate) keyboard_line: String,
    /// The line it writes at the hide, giving the query's length
    /// (`--query-line`).
    pub(crate) query_line: QueryLine,
}

impl Typing {
    /// How many keys the text is.
    fn keys(&self) -> usize {
        self.text.chars().count()
    }
}

/// `--type`'s text: one key a character, each one a key types on a US
/// layout without a modifier (as the typist types it), and at least one.
///
/// # Errors
///
/// A message naming the character no key types, or saying the text is
/// empty.
pub(crate) fn parse_text(text: &str) -> Result<String, String> {
    if text.is_empty() {
        return Err("there is nothing to type".into());
    }
    match text.chars().find(|&c| Key::typing(c).is_none()) {
        Some(c) => Err(format!(
            "{c:?} cannot be typed without a modifier on a US layout"
        )),
        None => Ok(text.to_owned()),
    }
}

/// `--query-line`: the words of the line the program writes at the hide,
/// with `{}` where it gives how many characters its query had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryLine {
    /// The words before the length.
    before: String,
    /// The words after it.
    after: String,
}

impl QueryLine {
    /// The pattern `pattern`, which holds `{}` once.
    ///
    /// # Errors
    ///
    /// A message when `pattern` holds no `{}`, or more than one.
    pub(crate) fn parse(pattern: &str) -> Result<Self, String> {
        match pattern.split_once("{}") {
            Some((before, after)) if !after.contains("{}") => Ok(Self {
                before: before.to_owned(),
                after: after.to_owned(),
            }),
            _ => Err(format!(
                "{pattern:?} must hold {{}} once, where the line gives the length"
            )),
        }
    }

    /// The length `line` gives, if it is such a line.
    fn length(&self, line: &str) -> Option<usize> {
        let (_, rest) = line.split_once(&self.before)?;
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let (number, tail) = rest.split_at(digits);
        tail.starts_with(&self.after)
            .then(|| number.parse().ok())
            .flatten()
    }

    /// The pattern, as given.
    fn pattern(&self) -> String {
        format!("{}{{}}{}", self.before, self.after)
    }
}

/// Why a typed show could not be measured.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TypingError {
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
    /// A key led to no frame.
    #[error(
        "{program} presented no frame for key {key} of {of} ({typed:?}) in show {show} \
         (waited {waited:?})\nIts last output:\n{output}"
    )]
    NoFrame {
        program: String,
        show: u32,
        key: usize,
        of: usize,
        typed: char,
        waited: Duration,
        output: String,
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
    /// The typist could not be run, or stopped.
    #[error("the typist (cargo xtask type) {what}{}", stderr_of(.stderr))]
    Typist { what: String, stderr: String },
}

fn stderr_of(stderr: &str) -> String {
    if stderr.trim().is_empty() {
        String::new()
    } else {
        format!("; it said:\n{}", stderr.trim_end())
    }
}

/// After show `show`'s first frame: waits for the program to say a key
/// typed reaches it (in its output after the first `mark` lines, those
/// before the summon), lets it settle, and types `typing`'s text
/// ([`type_query`]).
pub(crate) fn after_show(
    run: &mut Run,
    running: &mut Running<'_>,
    typist: &mut Typist,
    typing: &Typing,
    meter: &mut dyn Meter,
    mark: usize,
    show: u32,
) -> Result<(), Error> {
    wait_keyboard_line(running, typing, mark, show)?;
    running.settle()?;
    type_query(run, running, typist, typing, meter, show)
}

/// Waits, within the running program's timeout, for it to write
/// `typing`'s keyboard line after the first `mark` lines of its output.
fn wait_keyboard_line(
    running: &mut Running<'_>,
    typing: &Typing,
    mark: usize,
    show: u32,
) -> Result<(), Error> {
    let found = wait_line(running, mark, |line| line.contains(&typing.keyboard_line))?;
    found.map(drop).ok_or_else(|| {
        TypingError::NoKeyboardLine {
            program: running.program.to_owned(),
            show,
            line: typing.keyboard_line.clone(),
            waited: running.timeout,
            output: running.watch.tail(),
        }
        .into()
    })
}

/// Checks, after a hide, that the program said its query had as many
/// characters as `typing`'s text, in a line after the first `mark`.
pub(crate) fn check_query_line(
    running: &mut Running<'_>,
    typing: &Typing,
    mark: usize,
    show: u32,
) -> Result<(), Error> {
    let found = wait_line(running, mark, |line| {
        typing.query_line.length(line).is_some()
    })?;
    let Some(line) = found else {
        return Err(TypingError::NoQueryLine {
            program: running.program.to_owned(),
            show,
            pattern: typing.query_line.pattern(),
            waited: running.timeout,
            output: running.watch.tail(),
        }
        .into());
    };
    let said = typing.query_line.length(&line).unwrap_or_default();
    if said == typing.keys() {
        Ok(())
    } else {
        Err(TypingError::WrongLength {
            program: running.program.to_owned(),
            show,
            typed: typing.keys(),
            said,
            output: running.watch.tail(),
        }
        .into())
    }
}

/// The first line after the first `mark` that `wanted` takes, waited for
/// within the running program's timeout; `None` if none came.
fn wait_line(
    running: &mut Running<'_>,
    mark: usize,
    wanted: impl Fn(&str) -> bool,
) -> Result<Option<String>, Error> {
    let start = Instant::now();
    loop {
        let lines = running.watch.lines();
        if let Some(line) = lines
            .get(mark..)
            .unwrap_or_default()
            .iter()
            .find(|l| wanted(l))
        {
            return Ok(Some(line.clone()));
        }
        if start.elapsed() >= running.timeout {
            return Ok(None);
        }
        interrupt::check()?;
        running.alive()?;
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Types `typing`'s text into the shown program a key at a time, each key
/// followed by a frame and then the settle, and records, for show `show`,
/// what `meter` measured of each key and of them all, and the frames.
fn type_query(
    run: &mut Run,
    running: &mut Running<'_>,
    typist: &mut Typist,
    typing: &Typing,
    meter: &mut dyn Meter,
    show: u32,
) -> Result<(), Error> {
    let keys = typing.keys();
    let frames_before = running.watch.frames();
    let mut total = 0;
    for (index, typed) in typing.text.chars().enumerate() {
        let key = index + 1;
        let frames = running.watch.frames();
        meter.start()?;
        typist.key(typed)?;
        if !frame_after(running, frames)? {
            return Err(TypingError::NoFrame {
                program: running.program.to_owned(),
                show,
                key,
                of: keys,
                typed,
                waited: running.timeout,
                output: running.watch.tail(),
            }
            .into());
        }
        running.settle()?;
        let cost = meter.stop(&format!("show-{show}-key-{key}"))?;
        run.push(
            format!("show {show}: key {key} of {keys}: {}", meter.what()),
            meter.unit(),
            cost,
        );
        total += cost;
    }
    run.push(
        format!(
            "show {show}: typing {keys} characters, the keys' sum: {}",
            meter.what()
        ),
        meter.unit(),
        total,
    );
    run.push(
        format!("show {show}: typing: frames presented"),
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

#[cfg(test)]
mod tests;

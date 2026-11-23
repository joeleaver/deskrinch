//! `--type`: a query typed into a shown program, one key at a time
//! ([`super::steps`]).
//!
//! What measure cannot see from outside, the program says in its output,
//! in words `--type` is given and holds none of itself
//! ([`Typing::keyboard_line`], [`QueryLine`]): that a key typed now reaches
//! where it types, which the first key waits for, and at the hide how many
//! characters it was typed, which must be the text's. A key lost would
//! otherwise make the figure that of a shorter query, without saying so.

use std::time::{Duration, Instant};

use deskrinch_test_input::Key;

use super::super::report::Run;
use super::super::settle::Running;
use super::steps::{Measured, Rows, Scope, measure_steps};
use super::{InputClient, MeasuredInputError, Meter};
use crate::error::Error;
use crate::input::Step;
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
/// layout without a modifier (as the input client types it), and at least
/// one.
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
    pub(super) fn length(&self, line: &str) -> Option<usize> {
        let (_, rest) = line.split_once(&self.before)?;
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let (number, tail) = rest.split_at(digits);
        tail.starts_with(&self.after)
            .then(|| number.parse().ok())
            .flatten()
    }

    /// The pattern, as given.
    pub(super) fn pattern(&self) -> String {
        format!("{}{{}}{}", self.before, self.after)
    }
}

/// After show `show`'s first frame: waits for the program to say a key
/// typed reaches it (in its output after the first `mark` lines, those
/// before the summon), lets it settle, and types `typing`'s text a key at a
/// time.
pub(crate) fn after_show(
    run: &mut Run,
    running: &mut Running<'_>,
    client: &mut InputClient,
    typing: &Typing,
    meter: &mut dyn Meter,
    mark: usize,
    show: u32,
) -> Result<(), Error> {
    wait_keyboard_line(running, typing, mark, show)?;
    let settled = running.settle()?;
    super::super::settle::record(run, &format!("show {show}: before typing"), settled);
    let keys = typing.keys();
    let steps: Vec<Measured<Step>> = typing
        .text
        .chars()
        .enumerate()
        .map(|(index, typed)| Measured {
            action: Step::Key(typed),
            row: format!("key {} of {keys}", index + 1),
            named: format!("key {} of {keys} ({typed:?})", index + 1),
            dump: format!("show-{show}-key-{}", index + 1),
        })
        .collect();
    let prefix = format!("show {show}: ");
    let during = format!(" in show {show}");
    let sum = format!("typing {keys} characters, the keys' sum");
    let rows = Rows {
        prefix: &prefix,
        during: &during,
        from: "key",
        sum: &sum,
        frames: "typing: frames presented",
        scope: Scope::Main,
    };
    measure_steps(
        run,
        running,
        &mut |step| client.step(step),
        &steps,
        &rows,
        meter,
    )
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
        MeasuredInputError::NoKeyboardLine {
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
        return Err(MeasuredInputError::NoQueryLine {
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
        Err(MeasuredInputError::WrongLength {
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

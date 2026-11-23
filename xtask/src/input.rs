//! `cargo xtask input`: the input client `cargo xtask measure` drives, one
//! step at a time: keys typed (`--type`) and pointer moves
//! (`--place-pointer`, `--move-pointer`).
//!
//! It is hidden from `--help` because it is measure's: measure starts it as
//! a client of the nested session it measures in, and grants it the input
//! KWin restricts ([`grant`]). It is not for running by hand. Outside a
//! nested session it refuses, before it connects to anything: there it
//! would type into, and point at, the real desktop.
//!
//! # Protocol
//!
//! It makes a keyboard and a pointer (`deskrinch-test-input`) and says
//! [`READY`] on its standard output. Then it reads its standard input a
//! line at a time, each line one [`Step`]:
//!
//! - `key C`: types the one character C (a space included), pressed and
//!   released;
//! - `point OUTPUT X Y`: moves the pointer to (X, Y), in OUTPUT's logical
//!   pixels from its top-left corner, OUTPUT being the output's name;
//!
//! and says [`DONE`] once the compositor has handled it. It ends when its
//! input ends. Any other line, a character no key types, or a point off
//! its output ends it with an error.

use std::fmt;
use std::io::{BufRead, Write};
use std::path::Path;

use deskrinch_kwin_access::DesktopEntry;
use deskrinch_test_client::{Compositor, NotNested};
use deskrinch_test_input::{Input, KWIN_FAKE_INPUT_INTERFACE};

use crate::Outcome;
use crate::error::Error;

/// What the client says once its keyboard and pointer are made.
pub(crate) const READY: &str = "ready";
/// What the client says once a step has been handled.
pub(crate) const DONE: &str = "done";
/// The desktop id KWin grants the client under.
const GRANT_ID: &str = "org.deskrinch.MeasureInput";

/// One line of the client's input.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    /// A key typed: the one character it types.
    Key(char),
    /// The pointer moved to `(x, y)` on the output named `output`.
    Point {
        /// The output's name.
        output: String,
        /// Logical pixels from its left edge.
        x: f64,
        /// Logical pixels from its top edge.
        y: f64,
    },
}

impl Step {
    /// The step a line names.
    ///
    /// # Errors
    ///
    /// [`InputError::NotAStep`] for anything but `key C` or
    /// `point OUTPUT X Y`.
    pub(crate) fn parse(line: &str) -> Result<Self, InputError> {
        let not_a_step = || InputError::NotAStep(line.to_owned());
        if let Some(key) = line.strip_prefix("key ") {
            let mut chars = key.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(Self::Key(c)),
                _ => Err(not_a_step()),
            };
        }
        let rest = line.strip_prefix("point ").ok_or_else(not_a_step)?;
        let mut words = rest.split(' ');
        let (Some(output), Some(x), Some(y), None) =
            (words.next(), words.next(), words.next(), words.next())
        else {
            return Err(not_a_step());
        };
        let number = |word: &str| word.parse::<f64>().ok().filter(|n| n.is_finite());
        match (output.is_empty(), number(x), number(y)) {
            (false, Some(x), Some(y)) => Ok(Self::Point {
                output: output.to_owned(),
                x,
                y,
            }),
            _ => Err(not_a_step()),
        }
    }
}

/// The step's line, as [`Step::parse`] reads it.
impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(c) => write!(f, "key {c}"),
            Self::Point { output, x, y } => write!(f, "point {output} {x} {y}"),
        }
    }
}

/// Why the input client stopped.
#[derive(Debug, thiserror::Error)]
pub(crate) enum InputError {
    /// It was run outside a nested session.
    #[error(
        "`cargo xtask input` types and points only in a nested session xtask started, as \
         outside one it would reach the real desktop: {0}"
    )]
    NotNested(NotNested),
    /// Its devices could not be made, or a step not taken.
    #[error("the input client's keyboard or pointer")]
    Input(#[from] deskrinch_test_input::Error),
    /// A line of its input was not a step.
    #[error("a line of the input client's input is `key C` or `point OUTPUT X Y`, not {0:?}")]
    NotAStep(String),
    /// Its input or output failed.
    #[error("the input client's input or output")]
    Io(#[from] std::io::Error),
}

/// Takes the steps its standard input names, one a line, until it ends.
pub(crate) fn run() -> Result<Outcome, Error> {
    let input = std::io::stdin().lock();
    let output = std::io::stdout().lock();
    take_steps(input, output)?;
    Ok(Outcome::Passed)
}

fn take_steps(input: impl BufRead, mut output: impl Write) -> Result<(), InputError> {
    Compositor::in_session().map_err(InputError::NotNested)?;
    let mut devices = Input::connect()?;
    say(&mut output, READY)?;
    for line in input.lines() {
        match Step::parse(&line?)? {
            Step::Key(c) => devices.type_text(c.encode_utf8(&mut [0; 4]))?,
            Step::Point { output, x, y } => devices.move_pointer(&output, x, y)?,
        }
        say(&mut output, DONE)?;
    }
    Ok(())
}

fn say(output: &mut impl Write, what: &str) -> std::io::Result<()> {
    writeln!(output, "{what}")?;
    output.flush()
}

/// The desktop file that grants the client, run from `executable`, the
/// input KWin restricts.
pub(crate) fn grant(executable: &Path) -> Result<DesktopEntry, Error> {
    crate::grant::client(
        GRANT_ID,
        "deskrinch measure input",
        executable,
        &[KWIN_FAKE_INPUT_INTERFACE],
        &[],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_line_is_one_character_a_space_included() {
        assert_eq!(Step::parse("key p").unwrap(), Step::Key('p'));
        assert_eq!(Step::parse("key  ").unwrap(), Step::Key(' '));
        for line in ["key ", "key pl", "key", "p", ""] {
            assert!(
                matches!(Step::parse(line), Err(InputError::NotAStep(_))),
                "{line:?}"
            );
        }
    }

    #[test]
    fn a_point_line_names_its_output_and_where_on_it() {
        assert_eq!(
            Step::parse("point HEADLESS-2 640 12.5").unwrap(),
            Step::Point {
                output: "HEADLESS-2".into(),
                x: 640.0,
                y: 12.5
            }
        );
        for line in [
            "point HEADLESS-1 640",
            "point HEADLESS-1 640 12 3",
            "point HEADLESS-1 x 12",
            "point HEADLESS-1 inf 12",
            "point  640 12",
            "pointer HEADLESS-1 640 12",
        ] {
            assert!(
                matches!(Step::parse(line), Err(InputError::NotAStep(_))),
                "{line:?}"
            );
        }
    }

    #[test]
    fn a_step_is_written_as_it_is_read() {
        for step in [
            Step::Key(' '),
            Step::Key('t'),
            Step::Point {
                output: "Virtual-1".into(),
                x: 0.5,
                y: 700.0,
            },
        ] {
            assert_eq!(Step::parse(&step.to_string()).unwrap(), step);
        }
    }

    #[test]
    fn the_grant_is_the_input_kwin_restricts() {
        let entry = grant(Path::new("/build/debug/xtask")).unwrap();
        let contents = entry.contents();
        assert!(contents.contains("Exec=/build/debug/xtask"), "{contents}");
        assert!(contents.contains(KWIN_FAKE_INPUT_INTERFACE), "{contents}");
    }
}

//! `cargo xtask type`: the typist that `cargo xtask measure --type` drives,
//! one key at a time.
//!
//! It is hidden from `--help` because it is measure's: measure starts it as
//! a client of the nested session it measures in, and grants it the
//! keyboard KWin restricts ([`grant`]). It is not for running by hand.
//! Outside a nested session it refuses, before it connects to anything:
//! there it would type into the real desktop.
//!
//! It makes a keyboard (`deskrinch-test-input`) and says [`READY`] on its
//! standard output. Then it reads its standard input a line at a time,
//! each line one character: it types that key (pressed and released, once
//! the compositor has handled both) and says [`TYPED`]. It ends when its
//! input ends. A line that is not one character, or a character no key
//! types, ends it with an error.

use std::io::{BufRead, Write};
use std::path::Path;

use deskrinch_kwin_access::DesktopEntry;
use deskrinch_test_client::Compositor;
use deskrinch_test_input::{Input, KWIN_FAKE_INPUT_INTERFACE};

use crate::Outcome;
use crate::error::Error;

/// What the typist says once its keyboard is made.
pub(crate) const READY: &str = "ready";
/// What the typist says once a key has been typed.
pub(crate) const TYPED: &str = "typed";
/// The desktop id KWin grants the typist under.
const GRANT_ID: &str = "org.deskrinch.MeasureTypist";

/// Why the typist stopped.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TypistError {
    /// It was run outside a nested session.
    #[error(
        "`cargo xtask type` types only into a nested session xtask started \
         (DESKRINCH_TEST_COMPOSITOR is not set): outside one it would type into the real desktop"
    )]
    NotNested,
    /// Its keyboard could not be made, or a key typed.
    #[error("the typist's keyboard")]
    Input(#[from] deskrinch_test_input::Error),
    /// A line of its input was not one character.
    #[error("each line of the typist's input is one character, not {0:?}")]
    NotOneKey(String),
    /// Its input or output failed.
    #[error("the typist's input or output")]
    Io(#[from] std::io::Error),
}

/// Types the keys its standard input names, one a line, until it ends.
pub(crate) fn run() -> Result<Outcome, Error> {
    let input = std::io::stdin().lock();
    let output = std::io::stdout().lock();
    type_lines(input, output)?;
    Ok(Outcome::Passed)
}

fn type_lines(input: impl BufRead, mut output: impl Write) -> Result<(), TypistError> {
    if Compositor::nested().is_none() {
        return Err(TypistError::NotNested);
    }
    let mut keyboard = Input::connect()?;
    say(&mut output, READY)?;
    for line in input.lines() {
        let line = line?;
        keyboard.type_text(one_key(&line)?)?;
        say(&mut output, TYPED)?;
    }
    Ok(())
}

fn say(output: &mut impl Write, what: &str) -> std::io::Result<()> {
    writeln!(output, "{what}")?;
    output.flush()
}

/// `line` if it is one character.
fn one_key(line: &str) -> Result<&str, TypistError> {
    let mut chars = line.chars();
    match (chars.next(), chars.next()) {
        (Some(_), None) => Ok(line),
        _ => Err(TypistError::NotOneKey(line.to_owned())),
    }
}

/// The desktop file that grants the typist, run from `executable`, the
/// keyboard KWin restricts.
pub(crate) fn grant(executable: &Path) -> Result<DesktopEntry, Error> {
    crate::grant::client(
        GRANT_ID,
        "deskrinch measure typist",
        executable,
        &[KWIN_FAKE_INPUT_INTERFACE],
        &[],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_one_key() {
        assert_eq!(one_key("p").unwrap(), "p");
        assert_eq!(one_key(" ").unwrap(), " ");
        assert!(matches!(one_key(""), Err(TypistError::NotOneKey(_))));
        assert!(matches!(one_key("pl"), Err(TypistError::NotOneKey(_))));
    }

    #[test]
    fn the_grant_is_the_keyboard_kwin_restricts() {
        let entry = grant(Path::new("/build/debug/xtask")).unwrap();
        let contents = entry.contents();
        assert!(contents.contains("Exec=/build/debug/xtask"), "{contents}");
        assert!(contents.contains(KWIN_FAKE_INPUT_INTERFACE), "{contents}");
    }
}

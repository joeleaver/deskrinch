//! `[general]` `terminal`: the command line terminal applications run in.

use std::ffi::OsString;
use std::fmt;

use serde::{Deserialize, Serialize};

/// `terminal`: the terminal emulator's command line, the program and its
/// arguments (`["foot", "--"]`, `["konsole", "-e"]`), to which an
/// application's own command line is appended. Never empty, and its
/// program is never an empty string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
pub struct TerminalCommand(Vec<String>);

impl TerminalCommand {
    /// The program and its arguments.
    #[must_use]
    pub fn words(&self) -> &[String] {
        &self.0
    }
}

impl TryFrom<Vec<String>> for TerminalCommand {
    type Error = EmptyTerminal;

    fn try_from(words: Vec<String>) -> Result<Self, EmptyTerminal> {
        match words.first() {
            Some(program) if !program.is_empty() => Ok(Self(words)),
            _ => Err(EmptyTerminal),
        }
    }
}

impl From<TerminalCommand> for Vec<String> {
    fn from(command: TerminalCommand) -> Self {
        command.0
    }
}

/// A `terminal` that is empty, or whose program is an empty string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyTerminal;

impl fmt::Display for EmptyTerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a terminal command needs at least its program, such as [\"foot\", \"--\"]; \
             leave `terminal` out to use xdg-terminal-exec",
        )
    }
}

impl std::error::Error for EmptyTerminal {}

/// No terminal to run a terminal application in: `[general]` `terminal` is
/// not set, and the default, `xdg-terminal-exec`, is not installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoTerminal;

impl fmt::Display for NoTerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "this application runs in a terminal, and there is none: set `terminal` in \
             [general] (such as terminal = [\"foot\", \"--\"]), or install xdg-terminal-exec",
        )
    }
}

impl std::error::Error for NoTerminal {}

/// The command line terminal applications run in: `setting` when it is
/// set, else `default` (`xdg-terminal-exec` when it is installed; the
/// caller looks, as `deskrinch_desktop_entries::default_terminal` does).
///
/// # Errors
///
/// [`NoTerminal`] with neither.
pub fn terminal(
    setting: Option<&TerminalCommand>,
    default: Option<Vec<OsString>>,
) -> Result<Vec<OsString>, NoTerminal> {
    match setting {
        Some(command) => Ok(command.words().iter().map(OsString::from).collect()),
        None => default.ok_or(NoTerminal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(words: &[&str]) -> Result<TerminalCommand, EmptyTerminal> {
        TerminalCommand::try_from(words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn needs_a_program() {
        assert_eq!(command(&[]), Err(EmptyTerminal));
        assert_eq!(command(&["", "-e"]), Err(EmptyTerminal));
        let foot = command(&["foot", "--"]).unwrap();
        assert_eq!(foot.words(), ["foot", "--"]);
        assert_eq!(Vec::<String>::from(foot.clone()), ["foot", "--"]);
    }

    #[test]
    fn the_setting_wins_then_the_default_then_none() {
        let foot = command(&["foot", "--"]).unwrap();
        let exec = vec![OsString::from("xdg-terminal-exec")];
        let default = Some(exec.clone());
        assert_eq!(
            terminal(Some(&foot), default.clone()),
            Ok(vec![OsString::from("foot"), OsString::from("--")])
        );
        assert_eq!(terminal(None, default), Ok(exec));
        assert_eq!(terminal(None, None), Err(NoTerminal));
        assert!(NoTerminal.to_string().contains("`terminal` in [general]"));
    }
}

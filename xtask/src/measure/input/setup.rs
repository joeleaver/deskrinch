//! `--setup-command 'CMD'`: state made before the settle, so that memory
//! and the idle window are taken with it (a popup shown, a second
//! configuration copied in).
//!
//! Each command is run as a client of the session through `sh -c`, with
//! the session's environment, in the order given, after start-up's end and
//! before the settle, as a step command is ([`super::command`]); after each
//! the program is left to settle. Unlike a step it is not measured, and it
//! need not lead to a frame: what it led to (the frames on every window, the
//! windows opened and closed) is noted in the run. A command that ends with
//! a status other than 0, or does not end within the first-frame timeout,
//! fails the run, naming it (`setup command 2 of 3`) with its output.

use super::super::report::Run;
use super::super::settle::Running;
use super::command::{Commands, run_one};
use super::steps::{Scope, Seen};
use crate::error::Error;

/// Runs each setup command of `commands`, the program left to settle after
/// each, noting what each led to.
pub(crate) fn run_setup(
    run: &mut Run,
    running: &mut Running<'_>,
    commands: &Commands<'_>,
) -> Result<(), Error> {
    let count = commands.list.len();
    for (index, command) in commands.list.iter().enumerate() {
        let name = format!("setup command {} of {count}", index + 1);
        let mark = running.watch.lines().len();
        run_one(commands.shell, command, &name, running.timeout)?;
        running.settle()?;
        let lines = running.watch.lines();
        let seen = Seen::of(lines.get(mark..).unwrap_or_default(), Scope::Every);
        run.note(format!("{name}: windows"), seen.windows());
        run.note(
            format!("{name}: frames presented"),
            seen.presented.to_string(),
        );
    }
    Ok(())
}

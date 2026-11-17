//! What a run does to the program besides starting it and leaving it idle:
//! the setup commands, run before the settle (`--setup-command`); the
//! command steps, measured after the idle window (`--step-command`); and a
//! second memory reading once the steps are done (`--memory-after-steps`).

use std::process::Command;

use super::Request;
use super::input::Commands;

/// The commands and readings a run adds to its start and idle window.
pub(crate) struct Scripted<'a> {
    /// `--setup-command`: run after start-up's end, before the settle, not
    /// measured.
    pub(crate) setup: Option<Commands<'a>>,
    /// `--step-command`: measured steps after the idle window (and after
    /// the pointer's).
    pub(crate) steps: Option<Commands<'a>>,
    /// `--memory-after-steps`: memory read again once the steps are done.
    pub(crate) memory_after_steps: bool,
}

impl<'a> Scripted<'a> {
    /// What `request` asks, its commands run through `shell` (`sh` as a
    /// client of the run's session).
    pub(crate) fn of(request: &'a Request, shell: &'a dyn Fn() -> Command) -> Self {
        let commands = |list: &'a [String]| (!list.is_empty()).then_some(Commands { list, shell });
        Self {
            setup: commands(&request.setup_commands),
            steps: commands(&request.step_commands),
            memory_after_steps: request.memory_after_steps,
        }
    }
}

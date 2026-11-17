//! Requests measure refuses before any session starts, each saying why:
//! the options that cannot be measured together.

use super::Request;

/// Why a request cannot be measured.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Refusal {
    /// `--setup-command` with `--summon`.
    #[error(
        "--setup-command cannot be used with --summon: a --summon run measures the shows of a \
         program started hidden, and has no settle or idle window for a setup command's state to \
         be read in"
    )]
    SetupWithSummon,
    /// `--memory-after-steps` with `--instructions`.
    #[error(
        "--memory-after-steps cannot be used with --instructions: under Callgrind the program's \
         memory says nothing, so none is read"
    )]
    MemoryUnderCallgrind,
    /// `--memory-after-steps` with `--summon`.
    #[error(
        "--memory-after-steps cannot be used with --summon: a --summon run reads memory once, \
         hidden, before its first show"
    )]
    MemoryWithSummon,
    /// `--memory-after-steps` with no step to read after.
    #[error(
        "--memory-after-steps needs steps to read after: give --step-command or --move-pointer"
    )]
    MemoryWithoutSteps,
}

/// Whether `request` can be measured; the first reason it cannot.
pub(crate) fn check(request: &Request) -> Result<(), Refusal> {
    if !request.setup_commands.is_empty() && request.summons.is_some() {
        return Err(Refusal::SetupWithSummon);
    }
    if request.memory_after_steps {
        if request.instructions {
            return Err(Refusal::MemoryUnderCallgrind);
        }
        if request.summons.is_some() {
            return Err(Refusal::MemoryWithSummon);
        }
        if request.step_commands.is_empty() && request.pointer.is_none() {
            return Err(Refusal::MemoryWithoutSteps);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::show::Summons;
    use super::*;

    fn request() -> Request {
        Request::new("program".into())
    }

    #[test]
    fn setup_commands_are_refused_with_summon_and_say_why() {
        let mut asked = request();
        asked.setup_commands = vec!["true".to_owned()];
        assert_eq!(check(&asked), Ok(()));
        asked.summons = Some(Summons::new("show", "hide", 1));
        assert_eq!(check(&asked), Err(Refusal::SetupWithSummon));
        assert!(Refusal::SetupWithSummon.to_string().contains("no settle"));
    }

    #[test]
    fn memory_after_the_steps_needs_steps_and_a_timed_run() {
        let mut asked = request();
        asked.memory_after_steps = true;
        assert_eq!(check(&asked), Err(Refusal::MemoryWithoutSteps));
        asked.step_commands = vec!["true".to_owned()];
        assert_eq!(check(&asked), Ok(()));
        asked.instructions = true;
        assert_eq!(check(&asked), Err(Refusal::MemoryUnderCallgrind));
        asked.instructions = false;
        asked.summons = Some(Summons::new("show", "hide", 1));
        assert_eq!(check(&asked), Err(Refusal::MemoryWithSummon));
    }
}

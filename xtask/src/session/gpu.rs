//! Whether a session's programs may use the machine's GPU: one fact of the
//! run, which the session hands every client.
//!
//! A run that uses no GPU (`cargo xtask compositor-tests --renderer
//! software`, as CI runs it; `cargo xtask session --no-gpu`) is a GPU-less
//! machine's run on any machine: what its programs decide by the machine's
//! GPU, they decide as if it had none. The session says so in
//! [`VARIABLE`], set to `none` for its clients and unset otherwise. It is
//! xtask's to set, not a user's. `cargo xtask desktop` reads it for its
//! nested compositor's default renderer.

use std::process::Command;

/// The variable that tells a session's clients the run uses no GPU.
pub(crate) const VARIABLE: &str = "DESKRINCH_SESSION_GPU";
/// Its one value.
const NONE: &str = "none";

/// Whether a session's programs may use the machine's GPU.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Gpu {
    /// As the machine has one or not.
    #[default]
    Machine,
    /// Not at all: the run is a GPU-less machine's.
    None,
}

impl Gpu {
    /// Whether the run may use the GPU, as `allowed` says: the machine's,
    /// or none.
    pub(crate) fn from_allowed(allowed: bool) -> Self {
        if allowed { Self::Machine } else { Self::None }
    }

    /// The fact as the session that runs this program handed it down;
    /// [`Machine`](Self::Machine) outside a session.
    pub(crate) fn of_this_session() -> Self {
        Self::from_variable(std::env::var(VARIABLE).ok().as_deref())
    }

    fn from_variable(value: Option<&str>) -> Self {
        match value {
            Some(NONE) => Self::None,
            _ => Self::Machine,
        }
    }

    /// Whether a GPU is there to use, on a machine that has one
    /// (`machine_has_gpu`) or not.
    pub(crate) fn has_gpu(self, machine_has_gpu: bool) -> bool {
        self == Self::Machine && machine_has_gpu
    }

    /// Tells `command`, a client of the session, the fact.
    pub(crate) fn tell(self, command: &mut Command) {
        match self {
            Self::Machine => {
                command.env_remove(VARIABLE);
            }
            Self::None => {
                command.env(VARIABLE, NONE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    /// What a command is told: the variable's value, or "(removed)", or
    /// "(untouched)".
    fn told(gpu: Gpu) -> String {
        let mut command = Command::new("true");
        gpu.tell(&mut command);
        match command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new(VARIABLE))
        {
            Some((_, Some(value))) => value.to_string_lossy().into_owned(),
            Some((_, None)) => "(removed)".into(),
            None => "(untouched)".into(),
        }
    }

    #[test]
    fn clients_are_told_only_that_the_run_uses_no_gpu() {
        assert_eq!(told(Gpu::None), "none");
        // Removed, so a session inside another does not inherit it.
        assert_eq!(told(Gpu::Machine), "(removed)");
    }

    #[test]
    fn the_fact_comes_back_as_it_was_told() {
        assert_eq!(Gpu::from_variable(Some("none")), Gpu::None);
        assert_eq!(Gpu::from_variable(None), Gpu::Machine);
        assert_eq!(Gpu::from_variable(Some("")), Gpu::Machine);
        assert_eq!(Gpu::from_allowed(false), Gpu::None);
        assert_eq!(Gpu::from_allowed(true), Gpu::Machine);
    }

    #[test]
    fn a_gpu_is_there_only_on_the_machine_and_if_the_run_uses_it() {
        assert!(Gpu::Machine.has_gpu(true));
        assert!(!Gpu::Machine.has_gpu(false));
        assert!(!Gpu::None.has_gpu(true));
        assert!(!Gpu::None.has_gpu(false));
    }
}

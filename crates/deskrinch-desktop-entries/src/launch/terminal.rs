//! The terminal `Terminal=true` entries run in when the caller names none.

use std::ffi::OsString;

use crate::catalog::Program;
use crate::env::Environment;
use crate::scan::program_exists;

/// The freedesktop command that runs a command line in the user's preferred
/// terminal emulator.
pub const XDG_TERMINAL_EXEC: &str = "xdg-terminal-exec";

/// The terminal command to give [`LaunchContext::with_terminal`] when the
/// user named none: `xdg-terminal-exec` when it is on `env`'s `PATH`, which
/// takes the command line to run as its arguments. `None` without it:
/// there is no guessing among terminal emulators.
///
/// [`LaunchContext::with_terminal`]: crate::LaunchContext::with_terminal
#[must_use]
pub fn default_terminal(env: &Environment) -> Option<Vec<OsString>> {
    program_exists(Program::TryExec(XDG_TERMINAL_EXEC), &env.path)
        .then(|| vec![OsString::from(XDG_TERMINAL_EXEC)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn env_with_path(dir: &std::path::Path) -> Environment {
        let path = dir.as_os_str().to_owned();
        Environment::from_vars(move |name| (name == "PATH").then(|| path.clone()))
    }

    #[test]
    fn is_xdg_terminal_exec_when_it_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(default_terminal(&env_with_path(dir.path())), None);
        let program = dir.path().join(XDG_TERMINAL_EXEC);
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        // Not executable: not installed.
        assert_eq!(default_terminal(&env_with_path(dir.path())), None);
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            default_terminal(&env_with_path(dir.path())),
            Some(vec![OsString::from(XDG_TERMINAL_EXEC)])
        );
    }
}

//! A child of a refusal test, run outside any session where nothing could
//! be reached if the refusal failed.
//!
//! A test that a guard refuses must be harmless when the guard does not
//! refuse, as a test is run with its guard broken to show that it catches
//! that ("mutants of a guard are run only where the guard's failure is
//! harmless", `deskrinch-test-session`'s doc). The child is isolated as a
//! session would isolate it ([`isolate`](super::isolate)) on a temporary
//! root of its own, with no session there: no `WAYLAND_DISPLAY`,
//! `WAYLAND_SOCKET` or `DISPLAY`, an `XDG_RUNTIME_DIR` that is empty, a
//! session bus address under the root that leads nowhere, and none of the
//! variables an isolated session of ours names itself with. With the guard
//! gone, the child finds no compositor and no bus to connect to, and fails
//! there.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use deskrinch_test_session::{
    BUS_VARIABLE, COMPOSITOR_PID_VARIABLE, COMPOSITOR_VARIABLE, SESSION_DIR_VARIABLE,
    SESSION_GROUP_VARIABLE,
};
use tempfile::TempDir;

use super::{create_dirs, isolate};

/// The variables an isolated session of ours names itself with, which a
/// child outside any session must not have.
const SESSION_VARIABLES: [&str; 4] = [
    COMPOSITOR_VARIABLE,
    SESSION_DIR_VARIABLE,
    COMPOSITOR_PID_VARIABLE,
    SESSION_GROUP_VARIABLE,
];

/// Points `command` at nothing, as the module's doc says; the root it is
/// pointed under, which must outlive the child.
///
/// # Errors
///
/// When the temporary root or its directories cannot be made.
pub fn outside(command: &mut Command) -> io::Result<TempDir> {
    let root = tempfile::tempdir()?;
    create_dirs(root.path())?;
    isolate(command, root.path());
    for variable in SESSION_VARIABLES {
        command.env_remove(variable);
    }
    command.env(BUS_VARIABLE, nowhere(root.path()));
    Ok(root)
}

/// The session bus address [`outside`] gives a child under `root`: a socket
/// path in the root, where nothing listens.
#[must_use]
pub fn nowhere(root: &Path) -> String {
    format!("unix:path={}", no_bus(root).display())
}

/// The path [`nowhere`] names.
fn no_bus(root: &Path) -> PathBuf {
    root.join("no-bus")
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::fs;

    use super::*;

    /// The variables set (`Some`) or removed (`None`) on `command`.
    fn envs(command: &Command) -> Vec<(OsString, Option<OsString>)> {
        command
            .get_envs()
            .map(|(name, value)| (name.to_owned(), value.map(OsStr::to_owned)))
            .collect()
    }

    fn set(name: &str, value: impl Into<OsString>) -> (OsString, Option<OsString>) {
        (name.into(), Some(value.into()))
    }

    #[test]
    fn a_child_outside_has_no_display_no_session_and_a_bus_that_leads_nowhere() {
        let mut command = Command::new("true");
        command
            .env("WAYLAND_DISPLAY", "wayland-0")
            .env(COMPOSITOR_VARIABLE, deskrinch_test_session::KWIN)
            .env(BUS_VARIABLE, "unix:path=/run/user/1000/bus");
        let root = outside(&mut command).unwrap();
        let envs = envs(&command);
        for removed in ["WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DISPLAY"]
            .into_iter()
            .chain(SESSION_VARIABLES)
        {
            assert!(envs.contains(&(removed.into(), None)), "{removed}");
        }
        let runtime = root.path().join("run");
        assert!(envs.contains(&set("XDG_RUNTIME_DIR", runtime.clone())));
        assert_eq!(fs::read_dir(&runtime).unwrap().count(), 0, "not empty");
        assert!(envs.contains(&set(BUS_VARIABLE, nowhere(root.path()))));
        assert!(!no_bus(root.path()).exists(), "something is at the bus");
    }
}

//! An isolated session's environment: its own XDG directories under one
//! root, and none of the real session's display or bus variables.

use std::ffi::OsString;
use std::fs::DirBuilder;
use std::io;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::Path;
use std::process::Command;

/// Variables of the real session that would lead a program back to it.
pub const REAL_SESSION_VARIABLES: &[&str] = &[
    "DISPLAY",
    "XAUTHORITY",
    "WAYLAND_DISPLAY",
    "WAYLAND_SOCKET",
    "SWAYSOCK",
    "I3SOCK",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// The XDG base-directory variables, and the directory under the session's
/// root each one names.
pub const SESSION_DIRS: [(&str, &str); 5] = [
    ("XDG_RUNTIME_DIR", "run"),
    ("XDG_DATA_HOME", "data"),
    ("XDG_CONFIG_HOME", "config"),
    ("XDG_CACHE_HOME", "cache"),
    ("XDG_STATE_HOME", "state"),
];

/// Creates the session's directories under `root`, each one only its owner
/// may enter (as `XDG_RUNTIME_DIR` must be).
///
/// # Errors
///
/// When a directory cannot be created.
pub fn create_dirs(root: &Path) -> io::Result<()> {
    for (_, sub) in SESSION_DIRS {
        create_private_dir(&root.join(sub))?;
    }
    Ok(())
}

/// Creates the directory `path`, which only its owner may enter.
///
/// # Errors
///
/// When it cannot be created, also when it exists.
pub fn create_private_dir(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(path)
}

/// Points `command` at the session under `root` instead of the real one:
/// the real session's variables removed, the XDG directories set.
pub fn isolate(command: &mut Command, root: &Path) {
    for variable in REAL_SESSION_VARIABLES {
        command.env_remove(variable);
    }
    for (variable, sub) in SESSION_DIRS {
        command.env(variable, OsString::from(root.join(sub)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolation_replaces_the_session_variables() {
        let root = Path::new("/tmp/session");
        let mut command = Command::new("true");
        command.env("WAYLAND_DISPLAY", "wayland-real");
        isolate(&mut command, root);
        let envs: Vec<_> = command.get_envs().collect();
        for variable in REAL_SESSION_VARIABLES {
            assert!(
                envs.contains(&(variable.as_ref(), None)),
                "{variable} is not removed"
            );
        }
        assert!(envs.contains(&(
            "XDG_RUNTIME_DIR".as_ref(),
            Some(Path::new("/tmp/session/run").as_os_str())
        )));
    }

    #[test]
    fn the_directories_are_private() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = tempfile::tempdir().unwrap();
        create_dirs(root.path()).unwrap();
        for (_, sub) in SESSION_DIRS {
            let mode = std::fs::metadata(root.path().join(sub))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700, "{sub}");
        }
    }
}

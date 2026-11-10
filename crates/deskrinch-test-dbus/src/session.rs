//! An isolated session's environment, set explicitly rather than inherited:
//! its own home and XDG directories under one root, the specification's
//! default search paths, and none of the real session's display, bus or
//! login-session variables.
//!
//! What a program run with [`isolate`] sees:
//!
//! - `HOME` and the XDG base directories under the session's root
//!   ([`SESSION_DIRS`]);
//! - `XDG_CONFIG_DIRS` and `XDG_DATA_DIRS` at the XDG base-directory
//!   specification's defaults ([`SEARCH_PATHS`]), not the caller's, which
//!   can name directories in the real home;
//! - `CARGO_HOME` and `RUSTUP_HOME` at the real ones, so that cargo and
//!   rustup keep working with `HOME` moved;
//! - none of [`REAL_SESSION_VARIABLES`];
//! - everything else as the caller has it (`PATH`, the locale, `TERM`,
//!   `CARGO_*` and `RUST*` settings, ...).

use std::ffi::OsString;
use std::fs::DirBuilder;
use std::io;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::Path;
use std::process::Command;

/// Variables of the real session that would lead a program back to it (its
/// display, X authority, compositor, bus) or describe the real login
/// session (logind's session and seat, the display manager's objects, the
/// desktop it runs). A session that wants `XDG_CURRENT_DESKTOP` sets it
/// after [`isolate`].
pub const REAL_SESSION_VARIABLES: &[&str] = &[
    "DISPLAY",
    "XAUTHORITY",
    "WAYLAND_DISPLAY",
    "WAYLAND_SOCKET",
    "SWAYSOCK",
    "I3SOCK",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_ID",
    "XDG_SESSION_CLASS",
    "XDG_SESSION_DESKTOP",
    "XDG_SESSION_TYPE",
    "XDG_SESSION_PATH",
    "XDG_SEAT",
    "XDG_SEAT_PATH",
    "XDG_VTNR",
];

/// The home and XDG base-directory variables, and the directory under the
/// session's root each one names.
pub const SESSION_DIRS: [(&str, &str); 6] = [
    ("HOME", "home"),
    ("XDG_RUNTIME_DIR", "run"),
    ("XDG_DATA_HOME", "data"),
    ("XDG_CONFIG_HOME", "config"),
    ("XDG_CACHE_HOME", "cache"),
    ("XDG_STATE_HOME", "state"),
];

/// The XDG search paths, at the base-directory specification's defaults.
pub const SEARCH_PATHS: [(&str, &str); 2] = [
    ("XDG_CONFIG_DIRS", "/etc/xdg"),
    ("XDG_DATA_DIRS", "/usr/local/share/:/usr/share/"),
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

/// Every variable [`isolate`] sets for the session under `root`, with its
/// value: [`SESSION_DIRS`], [`SEARCH_PATHS`], and `CARGO_HOME` and
/// `RUSTUP_HOME` at this process's (or their defaults under its `HOME`).
#[must_use]
pub fn environment(root: &Path) -> Vec<(&'static str, OsString)> {
    let real_home = std::env::var_os("HOME");
    let toolchain_home = |variable: &'static str, default: &str| {
        std::env::var_os(variable)
            .or_else(|| {
                real_home
                    .as_ref()
                    .map(|home| Path::new(home).join(default).into_os_string())
            })
            .map(|value| (variable, value))
    };
    SESSION_DIRS
        .iter()
        .map(|&(variable, sub)| (variable, root.join(sub).into_os_string()))
        .chain(
            SEARCH_PATHS
                .iter()
                .map(|&(variable, value)| (variable, OsString::from(value))),
        )
        .chain(toolchain_home("CARGO_HOME", ".cargo"))
        .chain(toolchain_home("RUSTUP_HOME", ".rustup"))
        .collect()
}

/// Points `command` at the session under `root` instead of the real one:
/// [`REAL_SESSION_VARIABLES`] removed, [`environment`] set.
pub fn isolate(command: &mut Command, root: &Path) {
    for variable in REAL_SESSION_VARIABLES {
        command.env_remove(variable);
    }
    command.envs(environment(root));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated(root: &Path) -> Vec<(OsString, Option<OsString>)> {
        let mut command = Command::new("true");
        command.env("WAYLAND_DISPLAY", "wayland-real");
        isolate(&mut command, root);
        command
            .get_envs()
            .map(|(name, value)| (name.to_owned(), value.map(ToOwned::to_owned)))
            .collect()
    }

    fn value(envs: &[(OsString, Option<OsString>)], name: &str) -> Option<OsString> {
        envs.iter()
            .find(|(variable, _)| variable == name)
            .and_then(|(_, value)| value.clone())
    }

    #[test]
    fn isolation_removes_the_real_session() {
        let envs = isolated(Path::new("/tmp/session"));
        for variable in REAL_SESSION_VARIABLES {
            assert!(
                envs.contains(&((*variable).into(), None)),
                "{variable} is not removed"
            );
        }
    }

    #[test]
    fn isolation_moves_home_and_the_xdg_directories() {
        let envs = isolated(Path::new("/tmp/session"));
        assert_eq!(value(&envs, "HOME"), Some("/tmp/session/home".into()));
        assert_eq!(
            value(&envs, "XDG_RUNTIME_DIR"),
            Some("/tmp/session/run".into())
        );
        assert_eq!(value(&envs, "XDG_CONFIG_DIRS"), Some("/etc/xdg".into()));
        assert_eq!(
            value(&envs, "XDG_DATA_DIRS"),
            Some("/usr/local/share/:/usr/share/".into())
        );
    }

    #[test]
    fn cargo_and_rustup_keep_their_real_homes() {
        let envs = isolated(Path::new("/tmp/session"));
        let real = |variable: &str, default: &str| {
            std::env::var_os(variable).or_else(|| {
                std::env::var_os("HOME").map(|home| Path::new(&home).join(default).into())
            })
        };
        assert_eq!(value(&envs, "CARGO_HOME"), real("CARGO_HOME", ".cargo"));
        assert_eq!(value(&envs, "RUSTUP_HOME"), real("RUSTUP_HOME", ".rustup"));
        assert_ne!(
            value(&envs, "CARGO_HOME"),
            Some("/tmp/session/home/.cargo".into())
        );
    }

    #[test]
    fn the_environment_names_each_variable_once() {
        let environment = environment(Path::new("/tmp/session"));
        let mut names: Vec<_> = environment.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "{names:?}");
        assert!(
            names
                .iter()
                .all(|name| !REAL_SESSION_VARIABLES.contains(name)),
            "a variable is both removed and set"
        );
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

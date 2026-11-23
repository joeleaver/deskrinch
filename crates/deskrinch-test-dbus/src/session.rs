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
//! - none of the variables that lead an audio client to the real session's
//!   sound server: every one beginning with one of [`AUDIO_PREFIXES`]
//!   (`PULSE_`, `PIPEWIRE_`, `WIREPLUMBER_`, `SPA_`), and
//!   [`AUDIO_VARIABLES`] (`JACK_DEFAULT_SERVER`, `ALSA_CONFIG_PATH`);
//! - everything else as the caller has it (`PATH`, the locale, `TERM`,
//!   `CARGO_*` and `RUST*` settings, ...).
//!
//! `HOME` is the backstop for what ignores the XDG variables. A program
//! started from a file with a capability runs in secure mode, and KWin has
//! one (`cap_sys_nice`). In secure mode Mesa reads `XDG_CACHE_HOME` and
//! `MESA_SHADER_CACHE_DIR` through `secure_getenv`, gets nothing, and puts
//! its shader cache under `$HOME/.cache`; with `HOME` unset, under the home
//! of the user's passwd entry, the owner's own. So `HOME` under the
//! session's root is what keeps KWin's shader cache in the session.
//!
//! [`outside`] is the other side: a child of a refusal test pointed at no
//! session at all, so that it reaches nothing if the refusal fails.

use std::ffi::{OsStr, OsString};
use std::fs::DirBuilder;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

mod outside;

pub use outside::{nowhere, outside};

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

/// The prefixes of the variables that would lead an audio client to the real
/// session's sound server: its socket, cookie, configuration, default
/// devices and plugins (libpulse's `PULSE_SERVER` and `PULSE_COOKIE`,
/// PipeWire's `PIPEWIRE_REMOTE` and `PIPEWIRE_RUNTIME_DIR`, WirePlumber's and
/// SPA's configuration and plugin directories, ...). They are prefixes, not
/// names, because each family is open-ended and grows with new releases; the
/// underscore is part of the prefix, so `PULSEAUDIO` is left alone.
pub const AUDIO_PREFIXES: &[&str] = &["PULSE_", "PIPEWIRE_", "WIREPLUMBER_", "SPA_"];

/// Audio variables outside [`AUDIO_PREFIXES`]' families that name a sound
/// server or a configuration that does: JACK's server, which PipeWire's JACK
/// library also follows, and ALSA's configuration, where the PulseAudio and
/// PipeWire plugins are set up.
pub const AUDIO_VARIABLES: &[&str] = &["JACK_DEFAULT_SERVER", "ALSA_CONFIG_PATH"];

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

/// The directory the session under `root` gives `variable`, one of
/// [`SESSION_DIRS`]' (`XDG_DATA_HOME`, say); `None` for any other.
#[must_use]
pub fn dir(root: &Path, variable: &str) -> Option<PathBuf> {
    SESSION_DIRS
        .iter()
        .find(|(name, _)| *name == variable)
        .map(|(_, sub)| root.join(sub))
}

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

/// Whether an isolated session removes the variable `name`: one of
/// [`REAL_SESSION_VARIABLES`] or [`AUDIO_VARIABLES`], or one that begins
/// with one of [`AUDIO_PREFIXES`].
#[must_use]
pub fn is_removed(name: &OsStr) -> bool {
    REAL_SESSION_VARIABLES
        .iter()
        .chain(AUDIO_VARIABLES)
        .any(|variable| name == *variable)
        || AUDIO_PREFIXES
            .iter()
            .any(|prefix| name.as_bytes().starts_with(prefix.as_bytes()))
}

/// The variables an isolated session removes, for a program started from
/// this process: [`REAL_SESSION_VARIABLES`] and [`AUDIO_VARIABLES`], set or
/// not, and every variable of this process's environment that begins with
/// one of [`AUDIO_PREFIXES`]. For code that builds its own commands rather
/// than calling [`isolate`].
#[must_use]
pub fn to_remove() -> Vec<OsString> {
    removed(std::env::vars_os().map(|(name, _)| name))
}

/// The named variables, and those of `present` that [`is_removed`], each
/// once.
fn removed(present: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut names: Vec<OsString> = REAL_SESSION_VARIABLES
        .iter()
        .chain(AUDIO_VARIABLES)
        .map(OsString::from)
        .chain(present.into_iter().filter(|name| is_removed(name)))
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Points `command` at the session under `root` instead of the real one:
/// every variable [`is_removed`] says, whether this process has it or it
/// was set on `command`, removed; [`environment`] set.
pub fn isolate(command: &mut Command, root: &Path) {
    isolate_inheriting(command, root, std::env::vars_os().map(|(name, _)| name));
}

/// [`isolate`], with `inherited` the names of the environment `command`
/// would inherit.
fn isolate_inheriting(
    command: &mut Command,
    root: &Path,
    inherited: impl IntoIterator<Item = OsString>,
) {
    let set: Vec<OsString> = command
        .get_envs()
        .map(|(name, _)| name.to_owned())
        .collect();
    for variable in removed(inherited.into_iter().chain(set)) {
        command.env_remove(variable);
    }
    command.envs(environment(root));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variable_s_directory_is_the_one_the_session_gives_it() {
        let root = Path::new("/tmp/deskrinch-session-x");
        assert_eq!(dir(root, "XDG_DATA_HOME"), Some(root.join("data")));
        assert_eq!(dir(root, "XDG_CONFIG_HOME"), Some(root.join("config")));
        assert_eq!(dir(root, "XDG_STATE_HOME"), Some(root.join("state")));
        assert_eq!(dir(root, "PATH"), None);
        // The same as the session's environment says.
        for (variable, value) in environment(root) {
            if let Some(dir) = dir(root, variable) {
                assert_eq!(dir.into_os_string(), value, "{variable}");
            }
        }
    }

    fn isolated(root: &Path) -> Vec<(OsString, Option<OsString>)> {
        let mut command = Command::new("true");
        command.env("WAYLAND_DISPLAY", "wayland-real");
        isolate(&mut command, root);
        envs(&command)
    }

    fn envs(command: &Command) -> Vec<(OsString, Option<OsString>)> {
        command
            .get_envs()
            .map(|(name, value)| (name.to_owned(), value.map(ToOwned::to_owned)))
            .collect()
    }

    /// A variable of each audio family, and each named audio variable.
    const AUDIO_MARKED: &[&str] = &[
        "PULSE_SERVER",
        "PULSE_COOKIE",
        "PULSE_PROP_media.role",
        "PIPEWIRE_REMOTE",
        "PIPEWIRE_RUNTIME_DIR",
        "WIREPLUMBER_CONFIG_DIR",
        "SPA_PLUGIN_DIR",
        "JACK_DEFAULT_SERVER",
        "ALSA_CONFIG_PATH",
    ];

    /// Variables that only look like the audio families: the prefix
    /// without its underscore, or the family's name inside another.
    const NOT_AUDIO: &[&str] = &["PULSEAUDIO", "PULSE", "PIPEWIRE", "MY_PULSE_SERVER"];

    #[test]
    fn isolation_removes_the_audio_variables_set_on_the_command() {
        let mut command = Command::new("true");
        for variable in AUDIO_MARKED.iter().chain(NOT_AUDIO) {
            command.env(variable, "marker-from-outside");
        }
        isolate(&mut command, Path::new("/tmp/session"));
        let envs = envs(&command);
        for variable in AUDIO_MARKED {
            assert!(
                envs.contains(&((*variable).into(), None)),
                "{variable} is not removed"
            );
        }
        for variable in NOT_AUDIO {
            assert_eq!(
                value(&envs, variable),
                Some("marker-from-outside".into()),
                "{variable} is not the session's to remove"
            );
        }
    }

    #[test]
    fn isolation_removes_the_audio_variables_inherited() {
        let mut command = Command::new("true");
        let inherited = AUDIO_MARKED.iter().chain(NOT_AUDIO).map(OsString::from);
        isolate_inheriting(&mut command, Path::new("/tmp/session"), inherited);
        let envs = envs(&command);
        for variable in AUDIO_MARKED {
            assert!(
                envs.contains(&((*variable).into(), None)),
                "{variable} is not removed"
            );
        }
        for variable in NOT_AUDIO {
            assert!(
                !envs.iter().any(|(name, _)| name == variable),
                "{variable} is not the session's to remove"
            );
        }
    }

    #[test]
    fn the_prefix_rule_needs_the_underscore() {
        for prefix in AUDIO_PREFIXES {
            assert!(prefix.ends_with('_'), "{prefix}");
            assert!(is_removed(OsStr::new(prefix)), "{prefix}");
            assert!(is_removed(OsStr::new(&format!("{prefix}ANYTHING"))));
            let bare = prefix.trim_end_matches('_');
            assert!(!is_removed(OsStr::new(bare)), "{bare}");
            assert!(!is_removed(OsStr::new(&format!("{bare}X"))), "{bare}X");
        }
    }

    #[test]
    fn a_name_that_is_not_unicode_still_meets_the_prefix_rule() {
        use std::os::unix::ffi::OsStringExt as _;

        let name = OsString::from_vec(b"PULSE_\xff".to_vec());
        assert!(is_removed(&name));
        assert!(!is_removed(&OsString::from_vec(b"\xffPULSE_".to_vec())));
    }

    #[test]
    fn the_named_variables_are_removed_set_or_not() {
        let names = removed(std::iter::empty());
        for variable in REAL_SESSION_VARIABLES.iter().chain(AUDIO_VARIABLES) {
            assert!(names.contains(&(*variable).into()), "{variable}");
        }
        let names = removed(["PULSE_SERVER".into(), "PATH".into()]);
        assert!(names.contains(&"PULSE_SERVER".into()));
        assert!(!names.contains(&"PATH".into()));
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
            names.iter().all(|name| !is_removed(OsStr::new(name))),
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

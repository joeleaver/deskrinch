//! The environment the crate works in, captured once as a value.
//!
//! Nothing deeper in the crate reads process environment variables: the
//! caller builds an [`Environment`] (usually with
//! [`Environment::from_process`]) and passes it in, so tests run on
//! fixtures.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::locale::Locale;

/// Where desktop entries live and what the session looks like.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Environment {
    /// `$XDG_DATA_HOME`: the user's data directory, searched first.
    pub data_home: Option<PathBuf>,
    /// `$XDG_DATA_DIRS`: the system data directories, in precedence order.
    pub data_dirs: Vec<PathBuf>,
    /// `$XDG_CURRENT_DESKTOP`: the names of the running desktop, matched
    /// against `OnlyShowIn` and `NotShowIn`.
    pub current_desktops: Vec<String>,
    /// `$PATH`: where `TryExec` programs are looked for.
    pub path: Vec<PathBuf>,
    /// The preferred locales for messages, best first.
    pub locales: Vec<Locale>,
    /// The D-Bus address of the session bus, if there is one.
    pub session_bus: Option<String>,
}

impl Environment {
    /// Reads the environment of this process.
    #[must_use]
    pub fn from_process() -> Self {
        Self::from_vars(|name| std::env::var_os(name))
    }

    /// Builds the environment from `var`, which returns the value of an
    /// environment variable, by the XDG Base Directory rules:
    ///
    /// - `XDG_DATA_HOME` defaults to `$HOME/.local/share`, and
    ///   `XDG_DATA_DIRS` to `/usr/local/share:/usr/share`, when unset or
    ///   empty. Relative paths are invalid and ignored.
    /// - The locales come from `LC_ALL`, `LC_MESSAGES` or `LANG` (the first
    ///   set), preceded by the `LANGUAGE` list unless that locale is `C`,
    ///   as gettext does.
    /// - The session bus is `DBUS_SESSION_BUS_ADDRESS`, or the conventional
    ///   `$XDG_RUNTIME_DIR/bus`.
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let text = |name: &str| {
            var(name)
                .and_then(|value| value.into_string().ok())
                .filter(|value| !value.is_empty())
        };
        let home = var("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute());
        let data_home = var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| home.map(|home| home.join(".local/share")));
        let mut data_dirs = var("XDG_DATA_DIRS")
            .map(|dirs| absolute_paths(&dirs))
            .unwrap_or_default();
        if data_dirs.is_empty() {
            data_dirs = vec!["/usr/local/share".into(), "/usr/share".into()];
        }
        let current_desktops = text("XDG_CURRENT_DESKTOP")
            .map(|desktops| {
                desktops
                    .split(':')
                    .filter(|d| !d.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let path = var("PATH")
            .map(|path| absolute_paths(&path))
            .unwrap_or_default();
        let session_bus = text("DBUS_SESSION_BUS_ADDRESS")
            .or_else(|| text("XDG_RUNTIME_DIR").map(|dir| format!("unix:path={dir}/bus")));
        Self {
            data_home,
            data_dirs,
            current_desktops,
            path,
            locales: locales(&text),
            session_bus,
        }
    }

    /// Every data directory in precedence order: `data_home`, then
    /// `data_dirs`.
    pub fn data_dirs_in_order(&self) -> impl Iterator<Item = &Path> {
        self.data_home
            .iter()
            .chain(&self.data_dirs)
            .map(PathBuf::as_path)
    }
}

/// The absolute entries of a colon-separated path list.
fn absolute_paths(list: &std::ffi::OsStr) -> Vec<PathBuf> {
    std::env::split_paths(list)
        .filter(|dir| dir.is_absolute())
        .collect()
}

fn locales(text: &impl Fn(&str) -> Option<String>) -> Vec<Locale> {
    let Some(primary) = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(text)
        .and_then(|name| Locale::parse(&name))
    else {
        return Vec::new();
    };
    let mut locales: Vec<Locale> = text("LANGUAGE")
        .map(|list| list.split(':').filter_map(Locale::parse).collect())
        .unwrap_or_default();
    if !locales.contains(&primary) {
        locales.push(primary);
    }
    locales
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(vars: &[(&str, &str)]) -> Environment {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
            .collect();
        Environment::from_vars(|name| vars.get(name).cloned())
    }

    #[test]
    fn applies_the_base_directory_defaults() {
        let e = env(&[("HOME", "/home/u")]);
        assert_eq!(e.data_home, Some("/home/u/.local/share".into()));
        assert_eq!(
            e.data_dirs,
            [PathBuf::from("/usr/local/share"), "/usr/share".into()]
        );
        assert_eq!(env(&[]).data_home, None);
    }

    #[test]
    fn ignores_relative_directories() {
        let e = env(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "relative"),
            ("XDG_DATA_DIRS", "rel:/opt/share::/usr/share"),
        ]);
        assert_eq!(e.data_home, Some("/home/u/.local/share".into()));
        assert_eq!(
            e.data_dirs,
            [PathBuf::from("/opt/share"), "/usr/share".into()]
        );
        let order: Vec<_> = e.data_dirs_in_order().collect();
        assert_eq!(order[0], Path::new("/home/u/.local/share"));
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn reads_desktops_path_and_bus() {
        let e = env(&[
            ("XDG_CURRENT_DESKTOP", "deskrinch:KDE"),
            ("PATH", "/usr/bin:/bin"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]);
        assert_eq!(e.current_desktops, ["deskrinch", "KDE"]);
        assert_eq!(e.path, [PathBuf::from("/usr/bin"), "/bin".into()]);
        assert_eq!(
            e.session_bus.as_deref(),
            Some("unix:path=/run/user/1000/bus")
        );
        let e = env(&[("DBUS_SESSION_BUS_ADDRESS", "unix:path=/x")]);
        assert_eq!(e.session_bus.as_deref(), Some("unix:path=/x"));
    }

    #[test]
    fn orders_locales_like_gettext() {
        let names = |e: Environment| {
            e.locales
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(env(&[("LANG", "de_DE.UTF-8"), ("LANGUAGE", "fr:en_GB")])),
            ["fr", "en_GB", "de_DE"]
        );
        assert_eq!(
            names(env(&[
                ("LANG", "de_DE.UTF-8"),
                ("LC_MESSAGES", "sr_RS@latin")
            ])),
            ["sr_RS@latin"]
        );
        assert_eq!(
            names(env(&[("LC_ALL", "C"), ("LANGUAGE", "fr")])),
            Vec::<String>::new()
        );
    }
}

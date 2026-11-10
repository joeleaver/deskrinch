//! The XDG base directories, resolved by the [XDG Base Directory
//! Specification]'s rules from an environment the caller passes in.
//!
//! A [`BaseDirs`] is a plain value: where user data, configuration, state
//! and cache live, the system-wide data and configuration directories to
//! search after them, and the runtime directory. It is built from a function
//! that looks up environment variables ([`BaseDirs::from_vars`]), so the
//! rules run on fixtures; [`BaseDirs::from_process`] is that function applied
//! to this process's environment. Nothing here touches the file system: the
//! directories are where things belong, whether or not they exist yet.
//!
//! # The rules
//!
//! | Directory | Variable | Default when unset, empty or invalid |
//! |---|---|---|
//! | [`data_home`](BaseDirs::data_home) | `XDG_DATA_HOME` | `$HOME/.local/share` |
//! | [`config_home`](BaseDirs::config_home) | `XDG_CONFIG_HOME` | `$HOME/.config` |
//! | [`state_home`](BaseDirs::state_home) | `XDG_STATE_HOME` | `$HOME/.local/state` |
//! | [`cache_home`](BaseDirs::cache_home) | `XDG_CACHE_HOME` | `$HOME/.cache` |
//! | [`data_dirs`](BaseDirs::data_dirs) | `XDG_DATA_DIRS` | `/usr/local/share`, `/usr/share` |
//! | [`config_dirs`](BaseDirs::config_dirs) | `XDG_CONFIG_DIRS` | `/etc/xdg` |
//! | [`runtime_dir`](BaseDirs::runtime_dir) | `XDG_RUNTIME_DIR` | none |
//!
//! Every path must be absolute: a relative one is invalid and ignored, as
//! the specification requires. A list (`XDG_DATA_DIRS`, `XDG_CONFIG_DIRS`)
//! keeps its absolute entries in order and takes its default only when none
//! is left. A `$HOME`-based default needs `HOME` to be absolute; without it
//! the directory is `None`. The runtime directory has no default: the
//! specification leaves the fallback, and the warning it asks for, to the
//! application.
//!
//! # Example
//!
//! ```
//! use std::ffi::OsString;
//! use std::path::Path;
//! use deskrinch_xdg::BaseDirs;
//!
//! let dirs = BaseDirs::from_vars(|name| match name {
//!     "HOME" => Some(OsString::from("/home/u")),
//!     "XDG_CONFIG_DIRS" => Some(OsString::from("/etc/xdg/plasma:/etc/xdg")),
//!     _ => None,
//! });
//! let config: Vec<_> = dirs.config_paths("deskrinch/config.toml").collect();
//! assert_eq!(config[0], Path::new("/home/u/.config/deskrinch/config.toml"));
//! assert_eq!(config[2], Path::new("/etc/xdg/deskrinch/config.toml"));
//!
//! // This process's own environment:
//! let mine = BaseDirs::from_process();
//! # let _ = mine;
//! ```
//!
//! [XDG Base Directory Specification]: https://specifications.freedesktop.org/basedir/latest/

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The XDG base directories of one environment.
///
/// The `*_home` directories and the runtime directory are the user's own;
/// `data_dirs` and `config_dirs` are the system-wide ones searched after
/// them, most important first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BaseDirs {
    /// `$XDG_DATA_HOME`: the user's data files.
    pub data_home: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`: the user's configuration files.
    pub config_home: Option<PathBuf>,
    /// `$XDG_STATE_HOME`: state that persists between restarts but is not
    /// worth backing up (history, the layout last used).
    pub state_home: Option<PathBuf>,
    /// `$XDG_CACHE_HOME`: non-essential cached data.
    pub cache_home: Option<PathBuf>,
    /// `$XDG_DATA_DIRS`: the system data directories, most important first.
    pub data_dirs: Vec<PathBuf>,
    /// `$XDG_CONFIG_DIRS`: the system configuration directories, most
    /// important first.
    pub config_dirs: Vec<PathBuf>,
    /// `$XDG_RUNTIME_DIR`: the user's runtime files (sockets, pipes), if
    /// the session provides the directory.
    pub runtime_dir: Option<PathBuf>,
}

impl BaseDirs {
    /// The base directories of this process's environment.
    #[must_use]
    pub fn from_process() -> Self {
        Self::from_vars(|name| std::env::var_os(name))
    }

    /// The base directories by the specification's rules (see the
    /// [crate documentation](crate)), where `var` returns the value of an
    /// environment variable, or `None` when it is unset.
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let home = absolute(var("HOME"));
        let user_dir = |name: &str, default: &str| {
            absolute(var(name)).or_else(|| home.as_ref().map(|home| home.join(default)))
        };
        let system_dirs = |name: &str, defaults: &[&str]| {
            let dirs = var(name).map(|list| absolute_paths(&list));
            match dirs {
                Some(dirs) if !dirs.is_empty() => dirs,
                _ => defaults.iter().map(PathBuf::from).collect(),
            }
        };
        Self {
            data_home: user_dir("XDG_DATA_HOME", ".local/share"),
            config_home: user_dir("XDG_CONFIG_HOME", ".config"),
            state_home: user_dir("XDG_STATE_HOME", ".local/state"),
            cache_home: user_dir("XDG_CACHE_HOME", ".cache"),
            data_dirs: system_dirs("XDG_DATA_DIRS", &["/usr/local/share", "/usr/share"]),
            config_dirs: system_dirs("XDG_CONFIG_DIRS", &["/etc/xdg"]),
            runtime_dir: absolute(var("XDG_RUNTIME_DIR")),
        }
    }

    /// Every data directory, most important first: `data_home`, then
    /// `data_dirs`.
    pub fn data_dirs_in_order(&self) -> impl Iterator<Item = &Path> {
        in_order(self.data_home.as_ref(), &self.data_dirs)
    }

    /// Every configuration directory, most important first:
    /// `config_home`, then `config_dirs`.
    pub fn config_dirs_in_order(&self) -> impl Iterator<Item = &Path> {
        in_order(self.config_home.as_ref(), &self.config_dirs)
    }

    /// `relative` under every data directory, most important first: where
    /// to look for a data file. `relative` is a relative path such as
    /// `applications`; the files need not exist.
    pub fn data_paths(&self, relative: impl AsRef<Path>) -> impl Iterator<Item = PathBuf> {
        under(self.data_dirs_in_order(), relative)
    }

    /// `relative` under every configuration directory, most important
    /// first: where to look for a configuration file. `relative` is a
    /// relative path such as `deskrinch/config.toml`; the files need not
    /// exist.
    pub fn config_paths(&self, relative: impl AsRef<Path>) -> impl Iterator<Item = PathBuf> {
        under(self.config_dirs_in_order(), relative)
    }
}

/// `value` as a path, if it is an absolute one (an empty value is not).
fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

/// The absolute entries of a colon-separated path list, in order: how
/// `XDG_DATA_DIRS` and `XDG_CONFIG_DIRS` are read, and how a list such as
/// `PATH` is best read too. Relative and empty entries are dropped.
#[must_use]
pub fn absolute_paths(list: &OsStr) -> Vec<PathBuf> {
    std::env::split_paths(list)
        .filter(|dir| dir.is_absolute())
        .collect()
}

fn in_order<'a>(home: Option<&'a PathBuf>, dirs: &'a [PathBuf]) -> impl Iterator<Item = &'a Path> {
    home.into_iter().chain(dirs).map(PathBuf::as_path)
}

fn under<'a>(
    dirs: impl Iterator<Item = &'a Path>,
    relative: impl AsRef<Path>,
) -> impl Iterator<Item = PathBuf> {
    dirs.map(move |dir| dir.join(relative.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn dirs(vars: &[(&str, &str)]) -> BaseDirs {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
            .collect();
        BaseDirs::from_vars(|name| vars.get(name).cloned())
    }

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn applies_the_defaults() {
        let d = dirs(&[("HOME", "/home/u")]);
        assert_eq!(d.data_home, Some("/home/u/.local/share".into()));
        assert_eq!(d.config_home, Some("/home/u/.config".into()));
        assert_eq!(d.state_home, Some("/home/u/.local/state".into()));
        assert_eq!(d.cache_home, Some("/home/u/.cache".into()));
        assert_eq!(d.data_dirs, paths(&["/usr/local/share", "/usr/share"]));
        assert_eq!(d.config_dirs, paths(&["/etc/xdg"]));
        assert_eq!(d.runtime_dir, None);
    }

    #[test]
    fn takes_the_variables() {
        let d = dirs(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "/d"),
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_STATE_HOME", "/s"),
            ("XDG_CACHE_HOME", "/k"),
            ("XDG_DATA_DIRS", "/opt/share:/usr/share"),
            ("XDG_CONFIG_DIRS", "/etc/xdg/kde:/etc/xdg"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]);
        assert_eq!(d.data_home, Some("/d".into()));
        assert_eq!(d.config_home, Some("/c".into()));
        assert_eq!(d.state_home, Some("/s".into()));
        assert_eq!(d.cache_home, Some("/k".into()));
        assert_eq!(d.data_dirs, paths(&["/opt/share", "/usr/share"]));
        assert_eq!(d.config_dirs, paths(&["/etc/xdg/kde", "/etc/xdg"]));
        assert_eq!(d.runtime_dir, Some("/run/user/1000".into()));
    }

    #[test]
    fn ignores_relative_and_empty_paths() {
        let d = dirs(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "relative"),
            ("XDG_CONFIG_HOME", ""),
            ("XDG_DATA_DIRS", "rel:/opt/share::/usr/share"),
            ("XDG_CONFIG_DIRS", "rel:"),
            ("XDG_RUNTIME_DIR", "run"),
        ]);
        assert_eq!(d.data_home, Some("/home/u/.local/share".into()));
        assert_eq!(d.config_home, Some("/home/u/.config".into()));
        assert_eq!(d.data_dirs, paths(&["/opt/share", "/usr/share"]));
        assert_eq!(d.config_dirs, paths(&["/etc/xdg"]));
        assert_eq!(d.runtime_dir, None);
    }

    #[test]
    fn needs_an_absolute_home_for_the_home_defaults() {
        for d in [dirs(&[]), dirs(&[("HOME", "home")])] {
            assert_eq!(d.data_home, None);
            assert_eq!(d.config_home, None);
            assert_eq!(d.state_home, None);
            assert_eq!(d.cache_home, None);
            assert_eq!(d.config_dirs, paths(&["/etc/xdg"]));
        }
        let d = dirs(&[("XDG_CONFIG_HOME", "/c")]);
        assert_eq!(d.config_home, Some("/c".into()));
    }

    #[test]
    fn lists_directories_and_paths_in_precedence_order() {
        let d = dirs(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_DIRS", "/opt/share:/usr/share"),
        ]);
        let order: Vec<_> = d.data_dirs_in_order().collect();
        assert_eq!(
            order,
            [
                Path::new("/home/u/.local/share"),
                Path::new("/opt/share"),
                Path::new("/usr/share")
            ]
        );
        let apps: Vec<_> = d.data_paths("applications").collect();
        assert_eq!(apps[1], Path::new("/opt/share/applications"));
        let config: Vec<_> = d.config_paths("deskrinch/config.toml").collect();
        assert_eq!(
            config,
            paths(&[
                "/home/u/.config/deskrinch/config.toml",
                "/etc/xdg/deskrinch/config.toml"
            ])
        );
        let without_home = dirs(&[]);
        assert_eq!(without_home.config_dirs_in_order().count(), 1);
    }
}

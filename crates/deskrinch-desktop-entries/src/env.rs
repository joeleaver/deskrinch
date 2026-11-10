//! The environment the crate works in, captured once as a value.
//!
//! Nothing deeper in the crate reads process environment variables: the
//! caller builds an [`Environment`] (usually with
//! [`Environment::from_process`]) and passes it in, so tests run on
//! fixtures.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use deskrinch_xdg::{BaseDirs, absolute_paths};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_encode};

use crate::locale::Locale;

/// The bytes a D-Bus address value may hold unescaped
/// (`[-0-9A-Za-z_/.\*]`); every other byte is written `%XX`.
const ADDRESS_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'/')
    .remove(b'.')
    .remove(b'\\')
    .remove(b'*');

/// Where desktop entries live and what the session looks like.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Environment {
    /// The XDG base directories; entries are found in the data directories
    /// ([`BaseDirs::data_home`] first, then [`BaseDirs::data_dirs`]).
    pub base_dirs: BaseDirs,
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
    /// environment variable:
    ///
    /// - The base directories by the XDG Base Directory rules, as
    ///   [`BaseDirs::from_vars`] reads them.
    /// - The locales come from `LC_ALL`, `LC_MESSAGES` or `LANG` (the first
    ///   set), preceded by the `LANGUAGE` list unless that locale is `C`,
    ///   as gettext does.
    /// - The session bus is `DBUS_SESSION_BUS_ADDRESS`, or the conventional
    ///   `bus` socket in the runtime directory. Like every base directory,
    ///   the runtime directory counts only when it is absolute.
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let text = |name: &str| {
            var(name)
                .and_then(|value| value.into_string().ok())
                .filter(|value| !value.is_empty())
        };
        let base_dirs = BaseDirs::from_vars(&var);
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
        let session_bus = text("DBUS_SESSION_BUS_ADDRESS").or_else(|| {
            let runtime = base_dirs.runtime_dir.as_deref()?;
            Some(bus_address(&runtime.join("bus")))
        });
        Self {
            base_dirs,
            current_desktops,
            path,
            locales: locales(&text),
            session_bus,
        }
    }

    /// Every data directory in precedence order: `data_home`, then
    /// `data_dirs`.
    pub fn data_dirs_in_order(&self) -> impl Iterator<Item = &Path> {
        self.base_dirs.data_dirs_in_order()
    }
}

/// The D-Bus address of the socket at `path`, escaped as addresses require.
fn bus_address(path: &Path) -> String {
    let escaped = percent_encode(path.as_os_str().as_bytes(), ADDRESS_VALUE);
    format!("unix:path={escaped}")
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
    fn finds_entries_in_the_xdg_data_directories() {
        let e = env(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_DIRS", "rel:/opt/share::/usr/share"),
        ]);
        let order: Vec<_> = e.data_dirs_in_order().collect();
        assert_eq!(
            order,
            [
                Path::new("/home/u/.local/share"),
                Path::new("/opt/share"),
                Path::new("/usr/share")
            ]
        );
    }

    #[test]
    fn reads_desktops_path_and_bus() {
        let e = env(&[
            ("XDG_CURRENT_DESKTOP", "deskrinch:KDE"),
            ("PATH", "/usr/bin:bin::/bin"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]);
        assert_eq!(e.current_desktops, ["deskrinch", "KDE"]);
        assert_eq!(e.path, [PathBuf::from("/usr/bin"), "/bin".into()]);
        assert_eq!(
            e.session_bus.as_deref(),
            Some("unix:path=/run/user/1000/bus")
        );
        let e = env(&[
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/x"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]);
        assert_eq!(e.session_bus.as_deref(), Some("unix:path=/x"));
    }

    #[test]
    fn takes_the_bus_only_from_an_absolute_runtime_directory() {
        // The rule every base directory follows: a relative path is
        // invalid and ignored, so there is no conventional bus either.
        assert_eq!(
            env(&[("XDG_RUNTIME_DIR", "run/user/1000")]).session_bus,
            None
        );
        assert_eq!(env(&[("XDG_RUNTIME_DIR", "")]).session_bus, None);
        assert_eq!(env(&[]).session_bus, None);
    }

    #[test]
    fn escapes_the_bus_path_as_d_bus_addresses_require() {
        let e = env(&[("XDG_RUNTIME_DIR", "/run/a b,c;d=é")]);
        assert_eq!(
            e.session_bus.as_deref(),
            Some("unix:path=/run/a%20b%2Cc%3Bd%3D%C3%A9/bus")
        );
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

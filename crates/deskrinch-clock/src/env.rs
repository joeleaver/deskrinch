//! The environment the clock works in, captured once as a value.
//!
//! Nothing deeper in the crate reads process environment variables: the
//! caller builds an [`Environment`] (usually with
//! [`Environment::from_process`]) and passes it in, so tests run on
//! fixtures.

use std::ffi::OsString;
use std::path::PathBuf;

use icu_locale_core::Locale;

use crate::locale;

/// Where the time zone comes from and which locale formats the time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// `$TZ`, when set: an empty value, a time zone file (`:Europe/Berlin`,
    /// `Europe/Berlin`, `/path/to/file`) or a POSIX rule (`EST5EDT`). When
    /// unset, the time zone is [`localtime`](Self::localtime).
    pub tz: Option<String>,
    /// `$TZDIR`: the directory of time zone files that relative names in
    /// `TZ` and the target of `localtime` are looked up in.
    pub zoneinfo: PathBuf,
    /// The system time zone file, `/etc/localtime`: usually a symbolic link
    /// into `zoneinfo`, which `timedatectl set-timezone` replaces.
    pub localtime: PathBuf,
    /// The POSIX locale for times and dates: the first set of `LC_ALL`,
    /// `LC_TIME` and `LANG`, such as `de_DE.UTF-8`.
    pub time_locale: Option<String>,
}

impl Default for Environment {
    /// The system's time zone and the POSIX (`C`) locale.
    fn default() -> Self {
        Self {
            tz: None,
            zoneinfo: PathBuf::from(DEFAULT_ZONEINFO),
            localtime: PathBuf::from(DEFAULT_LOCALTIME),
            time_locale: None,
        }
    }
}

/// Where glibc and the tzdata packages put the time zone files.
const DEFAULT_ZONEINFO: &str = "/usr/share/zoneinfo";
/// The system time zone, as `systemd-timedated` and glibc know it.
const DEFAULT_LOCALTIME: &str = "/etc/localtime";

impl Environment {
    /// Reads the environment of this process.
    #[must_use]
    pub fn from_process() -> Self {
        Self::from_vars(|name| std::env::var_os(name))
    }

    /// Builds the environment from `var`, which returns the value of an
    /// environment variable, as glibc reads it:
    ///
    /// - `TZ` is kept as it is, even when empty (which means UTC). A value
    ///   that is not UTF-8 is ignored.
    /// - `TZDIR` replaces `/usr/share/zoneinfo` when it is an absolute path.
    /// - The time locale is `LC_ALL`, else `LC_TIME`, else `LANG`, skipping
    ///   empty values. `LANGUAGE` is not consulted: it orders translations
    ///   of messages, not the formats of times.
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let text = |name: &str| var(name).and_then(|value| value.into_string().ok());
        let zoneinfo = var("TZDIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .unwrap_or_else(|| PathBuf::from(DEFAULT_ZONEINFO));
        let time_locale = ["LC_ALL", "LC_TIME", "LANG"]
            .into_iter()
            .filter_map(text)
            .find(|value| !value.is_empty());
        Self {
            tz: text("TZ"),
            zoneinfo,
            localtime: PathBuf::from(DEFAULT_LOCALTIME),
            time_locale,
        }
    }

    /// The locale times and dates are formatted in:
    /// [`time_locale`](Self::time_locale) as a Unicode locale, by
    /// [`posix_to_unicode`](crate::posix_to_unicode).
    #[must_use]
    pub fn locale(&self) -> Locale {
        locale::posix_to_unicode(self.time_locale.as_deref().unwrap_or("C"))
    }
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
    fn defaults_to_the_system_zone_and_the_posix_locale() {
        let e = env(&[]);
        assert_eq!(e, Environment::default());
        assert_eq!(e.localtime, PathBuf::from("/etc/localtime"));
        assert_eq!(e.locale().to_string(), "und");
    }

    #[test]
    fn keeps_tz_even_when_empty() {
        assert_eq!(env(&[("TZ", "")]).tz.as_deref(), Some(""));
        assert_eq!(
            env(&[("TZ", ":Europe/Berlin")]).tz.as_deref(),
            Some(":Europe/Berlin")
        );
    }

    #[test]
    fn takes_an_absolute_tzdir() {
        assert_eq!(
            env(&[("TZDIR", "/opt/zi")]).zoneinfo,
            PathBuf::from("/opt/zi")
        );
        assert_eq!(
            env(&[("TZDIR", "zi")]).zoneinfo,
            PathBuf::from(DEFAULT_ZONEINFO)
        );
    }

    #[test]
    fn follows_the_posix_locale_chain() {
        let locale = |vars: &[(&str, &str)]| env(vars).time_locale;
        assert_eq!(
            locale(&[("LANG", "de_DE.UTF-8")]).as_deref(),
            Some("de_DE.UTF-8")
        );
        assert_eq!(
            locale(&[("LANG", "de_DE.UTF-8"), ("LC_TIME", "en_GB.UTF-8")]).as_deref(),
            Some("en_GB.UTF-8")
        );
        assert_eq!(
            locale(&[
                ("LANG", "de_DE.UTF-8"),
                ("LC_TIME", "en_GB.UTF-8"),
                ("LC_ALL", "fr_FR.UTF-8")
            ])
            .as_deref(),
            Some("fr_FR.UTF-8")
        );
        assert_eq!(
            locale(&[("LC_ALL", ""), ("LC_TIME", ""), ("LANG", "ja_JP.UTF-8")]).as_deref(),
            Some("ja_JP.UTF-8")
        );
        assert_eq!(
            locale(&[("LANG", "de_DE.UTF-8"), ("LANGUAGE", "fr")]).as_deref(),
            Some("de_DE.UTF-8")
        );
    }
}

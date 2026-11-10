//! The time zone, found and read the way glibc's `tzset` finds it.
//!
//! - `TZ` unset: the system time zone file, `/etc/localtime`. When it does
//!   not exist the zone is UTC, as for glibc.
//! - `TZ` empty: UTC.
//! - `TZ=:file`: the time zone file `file`, absolute or relative to the
//!   zoneinfo directory.
//! - `TZ=value`: the file `value` if there is one, else `value` as a POSIX
//!   rule such as `EST5EDT,M3.2.0,M11.1.0`.
//!
//! A zone that cannot be read is UTC, with the reason reported.
//!
//! A zone read from a file is named after it: a symbolic link into a
//! zoneinfo directory (as `/etc/localtime` usually is) gives the IANA name
//! after `zoneinfo/`, as systemd reads it; a file inside the zoneinfo
//! directory gives its relative path; any other file gives its own path.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jiff::tz::TimeZone;

use crate::env::Environment;
use crate::error::TimeZoneError;

/// A time zone, and what to watch for it changing.
#[derive(Debug)]
pub(crate) struct Resolved {
    /// The zone, UTC when it could not be read.
    pub(crate) zone: TimeZone,
    /// The file the zone is read from, or would be once it exists.
    pub(crate) file: Option<PathBuf>,
    /// Why the zone is UTC instead of what the environment names.
    pub(crate) error: Option<TimeZoneError>,
}

impl Resolved {
    fn new(zone: TimeZone, file: Option<PathBuf>) -> Self {
        Self {
            zone,
            file,
            error: None,
        }
    }

    fn failed(file: Option<PathBuf>, error: TimeZoneError) -> Self {
        Self {
            zone: TimeZone::UTC,
            file,
            error: Some(error),
        }
    }
}

/// Finds and reads the time zone `env` names.
pub(crate) fn resolve(env: &Environment) -> Resolved {
    let Some(tz) = env.tz.as_deref() else {
        let path = env.localtime.clone();
        return match read(&path, None, &env.zoneinfo) {
            Err(TimeZoneError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                Resolved::new(TimeZone::UTC, Some(path))
            }
            result => from_file(path, result),
        };
    };
    if tz.is_empty() {
        return Resolved::new(TimeZone::UTC, None);
    }
    let (file_only, spec) = match tz.strip_prefix(':') {
        Some(spec) => (true, spec),
        None => (false, tz),
    };
    let (path, name) = if Path::new(spec).is_absolute() {
        (PathBuf::from(spec), None)
    } else {
        (env.zoneinfo.join(spec), Some(spec))
    };
    let result = read(&path, name, &env.zoneinfo);
    if file_only || result.is_ok() {
        return from_file(path, result);
    }
    match TimeZone::posix(spec) {
        Ok(zone) => Resolved::new(zone, None),
        Err(source) => Resolved::failed(
            None,
            TimeZoneError::InvalidRule {
                rule: spec.to_owned(),
                source,
            },
        ),
    }
}

fn from_file(path: PathBuf, result: Result<TimeZone, TimeZoneError>) -> Resolved {
    match result {
        Ok(zone) => Resolved::new(zone, Some(path)),
        Err(error) => Resolved::failed(Some(path), error),
    }
}

/// Reads the TZif file at `path`, naming it `name` or by the rules in the
/// module documentation.
fn read(path: &Path, name: Option<&str>, zoneinfo: &Path) -> Result<TimeZone, TimeZoneError> {
    let data = fs::read(path).map_err(|source| TimeZoneError::Read {
        path: path.to_owned(),
        source,
    })?;
    let name = name.map_or_else(|| file_zone_name(path, zoneinfo), str::to_owned);
    TimeZone::tzif(&name, &data).map_err(|source| TimeZoneError::InvalidFile {
        path: path.to_owned(),
        source,
    })
}

fn file_zone_name(path: &Path, zoneinfo: &Path) -> String {
    let linked = fs::read_link(path).ok().and_then(|target| {
        let target = target.to_str()?;
        let (_, name) = target.rsplit_once("zoneinfo/")?;
        (!name.is_empty()).then(|| name.to_owned())
    });
    let inside = || {
        let real = fs::canonicalize(path).ok()?;
        let zoneinfo = fs::canonicalize(zoneinfo).ok()?;
        real.strip_prefix(zoneinfo)
            .ok()?
            .to_str()
            .map(str::to_owned)
    };
    linked
        .or_else(inside)
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_zones::{SYSTEM_ZONEINFO, ts};
    use std::os::unix::fs::symlink;

    fn env(tz: Option<&str>, localtime: &Path) -> Environment {
        Environment {
            tz: tz.map(str::to_owned),
            zoneinfo: PathBuf::from(SYSTEM_ZONEINFO),
            localtime: localtime.to_owned(),
            time_locale: None,
        }
    }

    fn offset_hours(zone: &TimeZone) -> i32 {
        zone.to_offset(ts("2026-01-15T12:00:00Z")).seconds() / 3600
    }

    #[test]
    fn reads_the_system_zone_through_its_link() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        symlink(format!("{SYSTEM_ZONEINFO}/Asia/Tokyo"), &localtime).unwrap();
        let resolved = resolve(&env(None, &localtime));
        assert!(resolved.error.is_none());
        assert_eq!(resolved.zone.iana_name(), Some("Asia/Tokyo"));
        assert_eq!(offset_hours(&resolved.zone), 9);
        assert_eq!(resolved.file, Some(localtime));
    }

    #[test]
    fn a_copied_system_zone_is_named_by_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        fs::copy(format!("{SYSTEM_ZONEINFO}/Asia/Tokyo"), &localtime).unwrap();
        let resolved = resolve(&env(None, &localtime));
        assert_eq!(resolved.zone.iana_name(), localtime.to_str());
        assert_eq!(offset_hours(&resolved.zone), 9);
    }

    #[test]
    fn no_system_zone_is_utc_and_watched_for() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        let resolved = resolve(&env(None, &localtime));
        assert!(resolved.error.is_none());
        assert_eq!(resolved.zone, TimeZone::UTC);
        assert_eq!(resolved.file, Some(localtime));
    }

    #[test]
    fn reads_tz_as_glibc_does() {
        let none = Path::new("/nonexistent");
        let named = resolve(&env(Some(":America/New_York"), none));
        assert_eq!(named.zone.iana_name(), Some("America/New_York"));
        assert_eq!(
            named.file,
            Some(PathBuf::from(SYSTEM_ZONEINFO).join("America/New_York"))
        );
        let bare = resolve(&env(Some("Europe/Berlin"), none));
        assert_eq!(bare.zone.iana_name(), Some("Europe/Berlin"));
        let absolute = resolve(&env(Some(&format!("{SYSTEM_ZONEINFO}/Asia/Tokyo")), none));
        assert_eq!(absolute.zone.iana_name(), Some("Asia/Tokyo"));
        let rule = resolve(&env(Some("XST-3XDT,M3.5.0,M10.5.0"), none));
        assert!(rule.error.is_none());
        assert_eq!(rule.file, None);
        assert_eq!(offset_hours(&rule.zone), 3);
        assert_eq!(resolve(&env(Some(""), none)).zone, TimeZone::UTC);
    }

    #[test]
    fn an_unreadable_zone_is_utc_with_the_reason() {
        let none = Path::new("/nonexistent");
        let missing = resolve(&env(Some(":Nowhere/Special"), none));
        assert_eq!(missing.zone, TimeZone::UTC);
        assert!(matches!(missing.error, Some(TimeZoneError::Read { .. })));
        let nonsense = resolve(&env(Some("not a zone"), none));
        assert!(matches!(
            nonsense.error,
            Some(TimeZoneError::InvalidRule { .. })
        ));

        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        fs::write(&localtime, b"not TZif").unwrap();
        let invalid = resolve(&env(None, &localtime));
        assert!(matches!(
            invalid.error,
            Some(TimeZoneError::InvalidFile { .. })
        ));
        assert_eq!(invalid.file, Some(localtime));
    }
}

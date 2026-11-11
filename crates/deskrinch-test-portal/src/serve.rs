//! The stand-in as a process of its own.

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::time::Duration;

use crate::file::{self, FileError};
use crate::{Error, Methods, Setting, StandIn, Value};

/// The variable naming the settings file (see [`parse`](crate::parse)) to
/// answer from, followed while the stand-in runs.
pub const SETTINGS_FILE: &str = "STAND_IN_FILE";

/// How often the settings file is read.
const FOLLOW: Duration = Duration::from_millis(100);

/// Serves the stand-in until the process is ended, configured by `var`
/// (the process's environment, usually):
///
/// - on the bus `DBUS_STARTER_ADDRESS` names (set by the bus when it
///   activates the stand-in), else `DBUS_SESSION_BUS_ADDRESS`;
/// - answering the settings in the file [`SETTINGS_FILE`] names (none while
///   it does not exist), and following it: a setting that changes or is
///   added is set and signalled (`SettingChanged`), one that is removed is
///   `NotFound` from then on. A file that is not a settings file is
///   reported on standard error, and the answers stay as they were.
///
/// # Errors
///
/// When the environment names no bus or no settings file, the file cannot
/// be read or is not a settings file when the stand-in starts, or the bus
/// fails.
pub fn serve_from_env(var: impl Fn(&str) -> Option<OsString>) -> Result<(), Error> {
    let address = var("DBUS_STARTER_ADDRESS")
        .or_else(|| var("DBUS_SESSION_BUS_ADDRESS"))
        .and_then(|address| address.into_string().ok())
        .ok_or_else(|| Error::Environment("no bus: DBUS_STARTER_ADDRESS is not set".into()))?;
    let file = var(SETTINGS_FILE)
        .ok_or_else(|| Error::Environment(format!("no settings: {SETTINGS_FILE} names no file")))?;
    let file = Path::new(&file);
    let mut answered = read(file).map_err(Error::Read)?;
    let portal = StandIn::start(&address, file::parse(&answered)?, Methods::Current)?;
    loop {
        std::thread::sleep(FOLLOW);
        let now = read(file).map_err(Error::Read)?;
        if now == answered {
            continue;
        }
        match changes(&answered, &now) {
            Ok(changes) => {
                for change in changes {
                    match change {
                        Change::Set(setting, value) => portal.set(setting, value)?,
                        Change::Remove(setting) => portal.remove(setting),
                    }
                }
                answered = now;
            }
            Err(error) => {
                eprintln!(
                    "deskrinch-test-portal: {}: {error}; answers kept",
                    file.display()
                );
            }
        }
    }
}

/// The file's text; empty while it does not exist.
fn read(file: &Path) -> io::Result<String> {
    match std::fs::read_to_string(file) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        other => other,
    }
}

/// What a change of the settings file does to the answers.
#[derive(Debug, PartialEq, Eq)]
enum Change<'a> {
    Set(Setting<'a>, Value),
    Remove(Setting<'a>),
}

/// The changes from the file `before` (one that was answered from, so it
/// parses) to `after`: the settings added or changed, then those removed.
fn changes<'a>(before: &'a str, after: &'a str) -> Result<Vec<Change<'a>>, FileError> {
    let after = last_values(file::parse(after)?);
    let before = last_values(file::parse(before).unwrap_or_default());
    let set = after
        .iter()
        .filter(|entry| !before.contains(entry))
        .map(|(setting, value)| Change::Set(*setting, value.clone()));
    let removed = before
        .iter()
        .filter(|(setting, _)| after.iter().all(|(now, _)| now != setting))
        .map(|(setting, _)| Change::Remove(*setting));
    Ok(set.chain(removed).collect())
}

/// Each setting once, with its last value, in the order they first appear.
fn last_values(settings: Vec<(Setting<'_>, Value)>) -> Vec<(Setting<'_>, Value)> {
    let mut last: Vec<(Setting<'_>, Value)> = Vec::with_capacity(settings.len());
    for (setting, value) in settings {
        match last.iter_mut().find(|(seen, _)| *seen == setting) {
            Some(entry) => entry.1 = value,
            None => last.push((setting, value)),
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{COLOR_SCHEME, GNOME_ICON_THEME, KDE_ICON_THEME};

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, OsString)> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn needs_a_bus_and_a_good_settings_file() {
        let no_bus = serve_from_env(vars(&[(SETTINGS_FILE, "/nonexistent")])).unwrap_err();
        assert!(matches!(no_bus, Error::Environment(message) if message.contains("no bus")));
        let bus = ("DBUS_STARTER_ADDRESS", "unix:path=/nonexistent");
        let no_file = serve_from_env(vars(&[bus])).unwrap_err();
        assert!(matches!(no_file, Error::Environment(message) if message.contains("no settings")));
        let dir = tempfile::tempdir().unwrap();
        let wrong = dir.path().join("settings");
        std::fs::write(&wrong, "org.freedesktop.appearance color-scheme dark\n").unwrap();
        let wrong = wrong.to_str().unwrap();
        let not_a_setting = serve_from_env(vars(&[bus, (SETTINGS_FILE, wrong)])).unwrap_err();
        assert!(matches!(
            not_a_setting,
            Error::File(FileError { line: 1, .. })
        ));
    }

    #[test]
    fn changes_are_the_settings_set_then_those_removed() {
        let before = "org.freedesktop.appearance color-scheme 1\n\
                      org.kde.kdeglobals.Icons Theme \"breeze\"\n\
                      org.gnome.desktop.interface icon-theme \"Adwaita\"\n";
        let after = "org.freedesktop.appearance color-scheme 1\n\
                     org.kde.kdeglobals.Icons Theme \"oxygen\"\n\
                     org.kde.kdeglobals.Icons Theme \"Papirus\"\n";
        assert_eq!(
            changes(before, after).unwrap(),
            [
                Change::Set(KDE_ICON_THEME, Value::text("Papirus")),
                Change::Remove(GNOME_ICON_THEME),
            ]
        );
        assert_eq!(
            changes("", "org.freedesktop.appearance color-scheme 2\n").unwrap(),
            [Change::Set(COLOR_SCHEME, Value::Number(2))]
        );
        assert!(changes(before, "half a line").is_err());
    }
}

//! The stand-in as a process of its own.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use crate::{Error, Methods, StandIn, number};

/// The variable naming the colour-scheme number to answer.
pub const SCHEME_VALUE: &str = "STAND_IN_SCHEME";
/// The variable naming a file whose number is the answer, followed while
/// the stand-in runs.
pub const SCHEME_FILE: &str = "STAND_IN_FILE";

/// How often a followed file is read.
const FOLLOW: Duration = Duration::from_millis(100);

/// Serves the stand-in until the process is ended, configured by `var`
/// (the process's environment, usually):
///
/// - on the bus `DBUS_STARTER_ADDRESS` names (set by the bus when it
///   activates the stand-in), else `DBUS_SESSION_BUS_ADDRESS`;
/// - answering the number in the file [`SCHEME_FILE`] names, and changing
///   (signalling `SettingChanged`) when the file does; else the number
///   [`SCHEME_VALUE`] names.
///
/// # Errors
///
/// When the environment names no bus or no answer, or the bus fails.
pub fn serve_from_env(var: impl Fn(&str) -> Option<OsString>) -> Result<(), Error> {
    let address = var("DBUS_STARTER_ADDRESS")
        .or_else(|| var("DBUS_SESSION_BUS_ADDRESS"))
        .and_then(|address| address.into_string().ok())
        .ok_or_else(|| Error::Environment("no bus: DBUS_STARTER_ADDRESS is not set".into()))?;
    if let Some(file) = var(SCHEME_FILE) {
        return follow(&address, Path::new(&file));
    }
    let scheme = var(SCHEME_VALUE)
        .and_then(|value| value.into_string().ok()?.trim().parse::<u32>().ok())
        .ok_or_else(|| Error::Environment(format!("no answer: {SCHEME_VALUE} is not a number")))?;
    let _portal = StandIn::start(&address, Some(number(scheme)), Methods::Current)?;
    loop {
        std::thread::park();
    }
}

/// Answers the number in `file`, following it.
fn follow(address: &str, file: &Path) -> Result<(), Error> {
    let read = || -> Option<u32> { std::fs::read_to_string(file).ok()?.trim().parse().ok() };
    let mut shown = read().unwrap_or(0);
    let portal = StandIn::start(address, Some(number(shown)), Methods::Current)?;
    loop {
        std::thread::sleep(FOLLOW);
        if let Some(now) = read().filter(|now| *now != shown) {
            portal.set(number(now))?;
            shown = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn needs_a_bus_and_an_answer() {
        let no_bus = serve_from_env(vars(&[(SCHEME_VALUE, "1")])).unwrap_err();
        assert!(matches!(no_bus, Error::Environment(message) if message.contains("no bus")));
        let no_answer = serve_from_env(vars(&[("DBUS_STARTER_ADDRESS", "unix:path=/nonexistent")]))
            .unwrap_err();
        assert!(matches!(no_answer, Error::Environment(message) if message.contains("no answer")));
        let not_a_number = serve_from_env(vars(&[
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent"),
            (SCHEME_VALUE, "dark"),
        ]))
        .unwrap_err();
        assert!(matches!(not_a_number, Error::Environment(_)));
    }
}

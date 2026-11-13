//! Starting the chosen application: with an xdg-activation token for the
//! launcher's window, so that the application's first window gets the
//! focus; in the terminal `[general]` `terminal` names when the application
//! runs in one; and off the UI thread, since a launch may wait on the bus
//! (D-Bus activation, a systemd scope).

use std::ffi::OsString;

use deskrinch_component::report;
use deskrinch_desktop_entries::{
    DesktopEntry, EntryKind, Environment, LaunchContext, default_terminal, launch,
};
use deskrinch_settings::general::{NoTerminal, TerminalCommand, terminal};
use rinch::prelude::{ActivationToken, ActivationTokenError};

use crate::starts::Starts;

/// The name the launched applications' systemd scopes are given.
const SCOPE_LAUNCHER: &str = "deskrinch-launcher";

/// Starts `entry` on a thread of its own, with `token` when there is one,
/// and in the terminal `setting` names (or the default) when it runs in
/// one; a start that succeeded is recorded in `starts`. What goes wrong is
/// reported, and not recorded.
pub(crate) fn start(
    entry: DesktopEntry,
    token: Result<ActivationToken, ActivationTokenError>,
    env: &Environment,
    setting: Option<TerminalCommand>,
    starts: Starts,
) {
    let mut context = LaunchContext::new(env).with_scope_launcher(SCOPE_LAUNCHER);
    match token {
        Ok(token) => context = context.with_activation_token(token.into_string()),
        // Started all the same; the compositor decides whether its window
        // takes the focus.
        Err(error) => tracing::info!(
            "starting {} without an activation token: {error}",
            entry.id.as_str()
        ),
    }
    let env = env.clone();
    let spawned = std::thread::Builder::new()
        .name("launcher-start".into())
        .spawn(move || {
            let what = format!("starting {}", entry.id.as_str());
            let context = match terminal_for(&entry, setting.as_ref(), || default_terminal(&env)) {
                Ok(Some(command)) => context.with_terminal(command),
                Ok(None) => context,
                Err(error) => {
                    report::error(&what, &error);
                    return;
                }
            };
            match launch(&entry, None, &[], &context) {
                Ok(_launched) => starts.record(entry.id),
                Err(error) => report::error(&what, &error),
            }
        });
    if let Err(error) = spawned {
        report::error("starting the launch thread", &error);
    }
}

/// The terminal `entry` runs in: none when it does not run in one (and
/// `default`, which looks on the `PATH`, is not asked); otherwise
/// `setting`, else `default`.
fn terminal_for(
    entry: &DesktopEntry,
    setting: Option<&TerminalCommand>,
    default: impl FnOnce() -> Option<Vec<OsString>>,
) -> Result<Option<Vec<OsString>>, NoTerminal> {
    let in_terminal = matches!(&entry.kind, EntryKind::Application(app) if app.terminal);
    if !in_terminal {
        return Ok(None);
    }
    terminal(setting, default()).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskrinch_desktop_entries::DesktopId;

    fn entry(terminal: bool) -> DesktopEntry {
        let text =
            format!("[Desktop Entry]\nType=Application\nName=Top\nExec=top\nTerminal={terminal}\n");
        let id = DesktopId::new("top.desktop").unwrap();
        DesktopEntry::parse(id, "/x".into(), &text).unwrap()
    }

    fn setting(words: &[&str]) -> TerminalCommand {
        TerminalCommand::try_from(words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>())
            .unwrap()
    }

    fn os(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn an_application_that_does_not_run_in_a_terminal_gets_none_without_looking() {
        let chosen = terminal_for(&entry(false), Some(&setting(&["foot"])), || {
            panic!("the PATH is not searched")
        });
        assert_eq!(chosen, Ok(None));
    }

    #[test]
    fn the_setting_comes_before_the_default() {
        let chosen = terminal_for(&entry(true), Some(&setting(&["foot", "--"])), || {
            Some(os(&["xdg-terminal-exec"]))
        });
        assert_eq!(chosen, Ok(Some(os(&["foot", "--"]))));
    }

    #[test]
    fn without_the_setting_the_default_is_used() {
        let chosen = terminal_for(&entry(true), None, || Some(os(&["xdg-terminal-exec"])));
        assert_eq!(chosen, Ok(Some(os(&["xdg-terminal-exec"]))));
    }

    #[test]
    fn with_neither_there_is_no_terminal() {
        assert_eq!(terminal_for(&entry(true), None, || None), Err(NoTerminal));
    }
}

//! Starting the chosen application: with an xdg-activation token for the
//! launcher's window, so that the application's first window gets the
//! focus, and off the UI thread, since a launch may wait on the bus (D-Bus
//! activation, a systemd scope).

use deskrinch_component::report;
use deskrinch_desktop_entries::{DesktopEntry, Environment, LaunchContext, launch};
use rinch::prelude::{ActivationToken, ActivationTokenError};

/// The name the launched applications' systemd scopes are given.
const SCOPE_LAUNCHER: &str = "deskrinch-launcher";

/// Starts `entry` on a thread of its own, with `token` when there is one.
/// What goes wrong is reported.
pub(crate) fn start(
    entry: DesktopEntry,
    token: Result<ActivationToken, ActivationTokenError>,
    env: &Environment,
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
    let spawned = std::thread::Builder::new()
        .name("launcher-start".into())
        .spawn(move || {
            if let Err(error) = launch(&entry, None, &[], &context) {
                report::error(&format!("starting {}", entry.id.as_str()), &error);
            }
        });
    if let Err(error) = spawned {
        report::error("starting the launch thread", &error);
    }
}

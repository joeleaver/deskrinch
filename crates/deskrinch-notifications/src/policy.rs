//! The daemon's choices, in one place: how it starts the service, and the
//! popups' sizes. Every rule of a notification's life (expiry, critical
//! urgency, replacement, closing, what invoking an action does) is the
//! service's (`deskrinch-notification-service`); here are only the values
//! the daemon gives it, and what the service leaves to the UI.

use std::time::Duration;

use deskrinch_notification_service::{Capabilities, Config, NameRequest};

/// How long a notification that leaves its timeout to the server (`-1`)
/// stays: the service's own default, said here so that the daemon's
/// choice is written once.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// What the popups offer, each matched to what they do:
///
/// - `actions`: each action is a button, the default one the popup itself.
/// - `action-icons`: an action whose key is an icon name is drawn as that
///   icon, when the sender asks for it.
/// - not `body-hyperlinks`: nothing opens a link yet, so the service makes
///   links plain text.
/// - not `sound`: nothing plays one.
pub(crate) const CAPABILITIES: Capabilities = Capabilities {
    actions: true,
    action_icons: true,
    body_hyperlinks: false,
    sound: false,
};

/// The widest the column grows, in logical pixels: a popup wraps its text
/// within it.
pub(crate) const MAX_WIDTH: u32 = 380;

/// The size a notification's icon, and an action's icon, are drawn at, in
/// logical pixels.
pub(crate) const ICON_SIZE: u32 = 32;

/// The lines of a body shown before it is cut short.
pub(crate) const BODY_LINES: u32 = 6;

/// The service's configuration: the session bus; the name only when it is
/// free, kept, and not waited for; the rest above.
pub(crate) fn config() -> Config {
    Config {
        name: NameRequest {
            replace_existing: false,
            allow_replacement: false,
            queue: false,
        },
        capabilities: CAPABILITIES,
        default_timeout: Some(DEFAULT_TIMEOUT),
        ..Config::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capabilities_reported_are_the_ones_built() {
        assert_eq!(
            config().capabilities.names(),
            [
                "body",
                "body-markup",
                "icon-static",
                "actions",
                "action-icons"
            ]
        );
    }

    #[test]
    fn the_name_is_taken_only_when_free_and_kept() {
        let name = config().name;
        assert!(!name.replace_existing);
        assert!(!name.allow_replacement);
        assert!(!name.queue);
    }
}

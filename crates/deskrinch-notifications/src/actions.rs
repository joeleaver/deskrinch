//! A popup's actions, and invoking one.
//!
//! - The default action (`"default"`) is the popup itself: a click on it
//!   invokes that action, when the sender offers one. It has no button.
//! - Every other action is a button, in the sender's order: its label, or,
//!   when the sender asks for action icons and the key is an icon name,
//!   that icon with the label as its accessible name.
//!
//! Invoking asks the compositor first for an activation token for the
//! sender (`rinch::prelude::request_activation_token`, from the click), so
//! that the window it raises may take the focus, then has the service
//! invoke the action with it. What follows (the signals, the popup closing
//! unless it is resident) is the service's.

use deskrinch_notification_service::{Action, Handle, IconName, Notification, NotificationId};
use rinch::prelude::request_activation_token;

/// One action button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Button {
    /// The key the sender gets back.
    pub(crate) key: String,
    /// The text, shown, or the icon's accessible name.
    pub(crate) label: String,
    /// The icon to draw instead of the text.
    pub(crate) icon: Option<IconName>,
}

/// The buttons of `notification`.
pub(crate) fn buttons(notification: &Notification) -> Vec<Button> {
    buttons_of(
        notification
            .actions
            .iter()
            .map(|action| (action.key.as_str(), action.label.as_str())),
        notification.action_icons,
    )
}

/// The buttons of the actions `(key, label)`, with `action_icons` asked for.
fn buttons_of<'a>(
    actions: impl Iterator<Item = (&'a str, &'a str)>,
    action_icons: bool,
) -> Vec<Button> {
    actions
        .filter(|(key, _)| *key != Action::DEFAULT_KEY)
        .map(|(key, label)| Button {
            key: key.to_owned(),
            label: label.to_owned(),
            icon: action_icons.then(|| IconName::parse(key)).flatten(),
        })
        .collect()
}

/// Invokes the action `key` of notification `id` through `handle`, from the
/// handler of the click that asked for it.
pub(crate) fn invoke(handle: Handle, id: NotificationId, key: String) {
    request_activation_token(move |answer| {
        let token = match answer {
            Ok(token) => Some(token.into_string()),
            Err(why) => {
                tracing::info!("no activation token for notification {id}'s action: {why}");
                None
            }
        };
        // It may have closed meanwhile (expired, or closed by its sender):
        // then there is nothing to invoke.
        if let Err(why) = handle.invoke_action(id, &key, token) {
            tracing::info!("the action {key:?} of notification {id} was not invoked: {why}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_action_has_no_button_and_the_rest_keep_their_order() {
        let actions = [
            ("reply", "Reply"),
            ("default", "Open"),
            ("archive", "Archive"),
        ];
        let keys: Vec<String> = buttons_of(actions.into_iter(), false)
            .into_iter()
            .map(|b| b.key)
            .collect();
        assert_eq!(keys, ["reply", "archive"]);
    }

    #[test]
    fn an_icon_only_when_asked_for_and_the_key_is_an_icon_name() {
        let actions = [("media-playback-pause", "Pause"), ("not an icon", "Other")];
        let without = buttons_of(actions.into_iter(), false);
        assert!(without.iter().all(|b| b.icon.is_none()));
        let with = buttons_of(actions.into_iter(), true);
        assert_eq!(with[0].icon, IconName::parse("media-playback-pause"));
        assert_eq!(with[0].label, "Pause");
        assert_eq!(with[1].icon, None);
    }
}

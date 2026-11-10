//! What the service advertises through `GetCapabilities`, in one place.

/// The optional features of the specification the service offers.
///
/// Some capabilities are what the model always does: `body` and
/// `body-markup` (the body is always parsed, see [`Body`](crate::Body)) and
/// `icon-static` (an image is one frame). The others depend on what the UI
/// shows or does, so the daemon sets them, and the model follows them: with
/// `actions` off, actions are dropped; with `body_hyperlinks` off, links are
/// plain text; with `action_icons` off, the hint is ignored.
///
/// Never advertised, deliberately: `body-images` (images in the body are not
/// loaded; the parser keeps their alternative text), `icon-multi` (no
/// animation), and `persistence` (the model holds live notifications only;
/// a history is the daemon's business).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each field is one independent capability of the specification, on or off"
)]
pub struct Capabilities {
    /// `actions`: the UI shows actions and lets the user invoke them.
    pub actions: bool,
    /// `action-icons`: the UI can show an action as an icon. Only advertised
    /// together with `actions`.
    pub action_icons: bool,
    /// `body-hyperlinks`: the UI shows links and lets the user follow them.
    pub body_hyperlinks: bool,
    /// `sound`: the daemon plays `sound-file` and `sound-name`, and honours
    /// `suppress-sound`.
    pub sound: bool,
}

impl Default for Capabilities {
    /// Everything but sound.
    fn default() -> Self {
        Self {
            actions: true,
            action_icons: true,
            body_hyperlinks: true,
            sound: false,
        }
    }
}

impl Capabilities {
    /// Whether `action-icons` is in effect: it needs `actions` as well.
    pub(crate) fn action_icons_in_effect(self) -> bool {
        self.actions && self.action_icons
    }

    /// The names `GetCapabilities` returns.
    #[must_use]
    pub fn names(self) -> Vec<&'static str> {
        let mut names = vec!["body", "body-markup", "icon-static"];
        let optional = [
            (self.actions, "actions"),
            (self.action_icons_in_effect(), "action-icons"),
            (self.body_hyperlinks, "body-hyperlinks"),
            (self.sound, "sound"),
        ];
        names.extend(
            optional
                .into_iter()
                .filter(|(on, _)| *on)
                .map(|(_, name)| name),
        );
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_names() {
        assert_eq!(
            Capabilities::default().names(),
            [
                "body",
                "body-markup",
                "icon-static",
                "actions",
                "action-icons",
                "body-hyperlinks"
            ]
        );
    }

    #[test]
    fn action_icons_need_actions() {
        let caps = Capabilities {
            actions: false,
            action_icons: true,
            body_hyperlinks: false,
            sound: true,
        };
        assert_eq!(
            caps.names(),
            ["body", "body-markup", "icon-static", "sound"]
        );
    }
}

//! What a connection can report and do: protocols differ, and the caller
//! asks rather than assumes.

use std::fmt;

use crate::window::WindowState;

/// An action on a window, requested with
/// [`WindowManagement::perform`](crate::WindowManagement::perform).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Action {
    /// Raise the window and give it keyboard focus.
    Activate,
    /// Minimize the window.
    Minimize,
    /// Undo [`Action::Minimize`].
    Restore,
    /// Ask the window to close (its application may ask the user first).
    Close,
}

impl Action {
    /// Every action, in declaration order.
    pub const ALL: [Self; 4] = [Self::Activate, Self::Minimize, Self::Restore, Self::Close];

    fn flag(self) -> Actions {
        match self {
            Self::Activate => Actions::ACTIVATE,
            Self::Minimize => Actions::MINIMIZE,
            Self::Restore => Actions::RESTORE,
            Self::Close => Actions::CLOSE,
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Activate => "activate",
            Self::Minimize => "minimize",
            Self::Restore => "restore",
            Self::Close => "close",
        })
    }
}

bitflags::bitflags! {
    /// A set of [`Action`]s.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Actions: u8 {
        /// [`Action::Activate`].
        const ACTIVATE = 1;
        /// [`Action::Minimize`].
        const MINIMIZE = 1 << 1;
        /// [`Action::Restore`].
        const RESTORE = 1 << 2;
        /// [`Action::Close`].
        const CLOSE = 1 << 3;
    }
}

/// The protocol a connection speaks, for diagnostics. Code should branch on
/// [`Capabilities`], not on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Protocol {
    /// `zwlr_foreign_toplevel_management_v1` (wlroots compositors, and
    /// deskrinch's own).
    WlrForeignToplevel,
    /// `org_kde_plasma_window_management` (KWin).
    PlasmaWindowManagement,
}

/// What a connection reports and which actions it can request.
///
/// A capability the connection lacks is absent here rather than failing at
/// use: a state flag it does not report is never set, outputs it does not
/// report stay empty, and an action it lacks returns
/// [`ActionError::Unsupported`](crate::ActionError::Unsupported). A
/// list-only protocol is a connection with no actions.
///
/// These say what the protocol, as this compositor offers it, can carry. A
/// compositor may still not use all of it: sway, for one, offers the
/// version of the wlroots protocol that reports parents but never reports
/// one, and accepts minimize requests while having no minimized state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// The protocol in use.
    pub protocol: Protocol,
    /// The state flags the connection reports.
    pub states: WindowState,
    /// Whether [`Window::outputs`](crate::Window::outputs) is reported. It is
    /// false when the compositor gives outputs no names (`wl_output` before
    /// version 4 and no `zxdg_output_v1` names), as the outputs could not be
    /// told apart.
    pub outputs: bool,
    /// Whether [`Window::parent`](crate::Window::parent) is reported.
    pub parent: bool,
    /// The actions the connection can request.
    pub actions: Actions,
}

impl Capabilities {
    /// Whether `action` can be requested on this connection.
    #[must_use]
    pub fn supports(&self, action: Action) -> bool {
        self.actions.contains(action.flag())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_reads_the_action_set() {
        let capabilities = Capabilities {
            protocol: Protocol::WlrForeignToplevel,
            states: WindowState::ACTIVE,
            outputs: true,
            parent: false,
            actions: Actions::ACTIVATE | Actions::CLOSE,
        };
        let supported: Vec<_> = Action::ALL
            .into_iter()
            .filter(|&action| capabilities.supports(action))
            .collect();
        assert_eq!(supported, [Action::Activate, Action::Close]);
    }

    #[test]
    fn every_action_has_its_own_flag() {
        let all = Action::ALL
            .into_iter()
            .fold(Actions::empty(), |set, action| set | action.flag());
        assert_eq!(all, Actions::all());
    }
}

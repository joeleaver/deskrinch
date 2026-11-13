//! What a connection can report and do: the protocols differ, and the
//! caller asks rather than assumes.

use std::fmt;

use crate::workspace::WorkspaceState;

/// An action on a workspace, requested with
/// [`Workspaces::perform`](crate::Workspaces::perform).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Action {
    /// Show the workspace (on its group's outputs).
    Activate,
}

impl Action {
    /// Every action, in declaration order.
    pub const ALL: [Self; 1] = [Self::Activate];

    pub(crate) fn flag(self) -> Actions {
        match self {
            Self::Activate => Actions::ACTIVATE,
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Activate => "activate",
        })
    }
}

bitflags::bitflags! {
    /// A set of [`Action`]s.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Actions: u8 {
        /// [`Action::Activate`].
        const ACTIVATE = 1;
    }
}

/// The protocol a connection speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Protocol {
    /// `ext_workspace_v1`, the standard protocol (labwc; deskrinch's own
    /// compositor).
    ExtWorkspace,
    /// `org_kde_plasma_virtual_desktop_management`, KWin's.
    PlasmaVirtualDesktop,
}

/// What a connection reports and which actions it can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// The protocol in use.
    pub protocol: Protocol,
    /// The state flags it reports. Others are never set.
    pub states: WorkspaceState,
    /// Whether groups name their outputs.
    pub outputs: bool,
    /// Whether workspaces may carry coordinates. The compositor may still
    /// send none (labwc does not).
    pub layout: bool,
    /// The actions the protocol can carry. Whether the compositor allows one
    /// on a given workspace is that workspace's own
    /// [`actions`](crate::Workspace::actions).
    pub actions: Actions,
}

impl Capabilities {
    /// Whether the protocol can carry `action`.
    #[must_use]
    pub fn supports(&self, action: Action) -> bool {
        self.actions.contains(action.flag())
    }
}

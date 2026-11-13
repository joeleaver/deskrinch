//! What a workspace is here, and the events that report them.

use std::collections::BTreeSet;
use std::fmt;

use crate::capabilities::Actions;
use crate::error::EndReason;

/// A workspace's identity for as long as the connection lasts: the
/// crate's own, not the compositor's, so that it is the same kind of value
/// whichever protocol carries the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId(pub(crate) u64);

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "workspace {}", self.0)
    }
}

/// A workspace group's identity for as long as the connection lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(pub(crate) u64);

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "group {}", self.0)
    }
}

bitflags::bitflags! {
    /// What a workspace is, as its protocol reports it. A flag the
    /// connection does not report ([`Capabilities::states`](crate::Capabilities::states))
    /// is never set.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct WorkspaceState: u8 {
        /// It is the one shown (on its group's outputs).
        const ACTIVE = 1;
        /// Something on it wants attention.
        const URGENT = 1 << 1;
        /// The compositor asks that it not be shown in a switcher.
        const HIDDEN = 1 << 2;
    }
}

/// A workspace (KWin calls it a virtual desktop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// Its identity.
    pub id: WorkspaceId,
    /// Its name, cleaned of control characters and bounded.
    pub name: String,
    /// Active, urgent, hidden.
    pub state: WorkspaceState,
    /// The group (and so the outputs) it belongs to, where the protocol has
    /// said.
    pub group: Option<GroupId>,
    /// Its place in the compositor's layout of workspaces, one coordinate
    /// per dimension (`[row, column]` for KWin's grid); empty where the
    /// compositor gives none.
    pub coordinates: Vec<u32>,
    /// What the compositor allows on it now.
    pub actions: Actions,
}

/// A set of workspaces that share outputs: where a compositor has
/// workspaces per output, as sway does, a group per output; where they are
/// shared by every output, as KWin's are, one group on all of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Its identity.
    pub id: GroupId,
    /// The names of the outputs it is on.
    pub outputs: BTreeSet<String>,
}

/// A change to the workspaces, delivered to the subscriber.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    /// A workspace appeared, or was there when the connection was made.
    Added(Workspace),
    /// A workspace changed; it carries the whole new state.
    Changed(Workspace),
    /// A workspace went away.
    Removed(WorkspaceId),
    /// A group appeared or its outputs changed; it carries the whole new
    /// state.
    GroupChanged(Group),
    /// A group went away.
    GroupRemoved(GroupId),
    /// The connection stopped reporting. Nothing follows it.
    Ended(EndReason),
}

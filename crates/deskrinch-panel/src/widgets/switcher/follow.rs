//! The workspaces followed: the switcher's model, kept where the workspaces
//! crate's events arrive, and its runs of buttons handed to the UI.
//!
//! Each event is applied to the model on the workspaces crate's thread, in
//! the order the subscriber hears them, under the model's lock, and the UI
//! is handed the runs that result, as a whole, through a [`Handover`], which
//! keeps the newest. A click takes the same lock to ask whether its
//! workspace may be activated.
//!
//! The workspaces are followed on the panel's shared connection, made off
//! the first frame's path ([`crate::compositor`]): the switcher is empty
//! until the compositor has answered. Where the compositor offers no
//! workspace protocol (sway), it stays empty, and one line at info level
//! says why: that is the switcher's expected state there, not a fault.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use deskrinch_component::{Handover, report};
use deskrinch_switcher::{Change, Entry, Run, Switcher};
use deskrinch_workspaces::{
    Action, Actions, Client, ConnectError, Event, GroupId, Workspace, WorkspaceId,
    WorkspaceManager, WorkspaceState, Workspaces, connect_with,
};
use rinch::prelude::Signal;

use crate::compositor::Follow;

/// The runs the UI draws.
pub(crate) type Runs = Arc<Vec<Run<WorkspaceId>>>;

/// The workspaces followed for the switcher.
pub(crate) struct Following {
    model: Arc<Mutex<Switcher<WorkspaceId, GroupId>>>,
    runs: Arc<Handover<Runs>>,
    /// `None` until connected, and when the compositor offers no workspace
    /// protocol.
    manager: Arc<Mutex<Option<WorkspaceManager>>>,
}

impl Following {
    /// The workspaces to follow, none until the panel's shared connection is
    /// handed to the [`Follow`] returned beside it ([`crate::compositor`]).
    pub(crate) fn new() -> (Self, Follow) {
        let model = Arc::new(Mutex::new(Switcher::new()));
        let runs: Arc<Handover<Runs>> = Arc::default();
        let manager = Arc::new(Mutex::new(None));
        let (follow_model, follow_runs, follow_manager) =
            (Arc::clone(&model), Arc::clone(&runs), Arc::clone(&manager));
        let connect: Follow = Box::new(move |client| {
            if let Some(client) = client {
                follow(client, &follow_model, &follow_runs, &follow_manager);
            }
        });
        let following = Self {
            model,
            runs,
            manager,
        };
        (following, connect)
    }

    /// The UI's signal of the runs, on the UI thread: none until the
    /// compositor has answered.
    pub(crate) fn signal(&self) -> Signal<Runs> {
        self.runs.signal(Arc::default())
    }

    /// A click on `id`'s button: asks the compositor to activate it, where
    /// it allows that. The mark moves when the compositor says it has.
    pub(crate) fn activate(&self, id: WorkspaceId) {
        if !lock(&self.model).activatable(id) {
            return;
        }
        if let Some(manager) = lock(&self.manager).as_ref()
            && let Err(error) = manager.perform(id, Action::Activate)
        {
            report::error(&format!("asking to activate {id}"), &error);
        }
    }
}

/// Follows the workspaces on `client`, applying each event to `model` and
/// handing `runs` what results, and keeps the connection in `manager`.
fn follow(
    client: &Client,
    model: &Arc<Mutex<Switcher<WorkspaceId, GroupId>>>,
    runs: &Arc<Handover<Runs>>,
    manager: &Mutex<Option<WorkspaceManager>>,
) {
    let applied = Arc::clone(model);
    let deliver = Arc::clone(runs);
    let connected = connect_with(client, move |event| {
        if let Event::Ended(reason) = &event {
            report::error("following the workspaces (the switcher is empty)", reason);
        }
        let Some(change) = change(event) else {
            return;
        };
        let mut model = lock(&applied);
        let before = model.runs();
        model.apply(change);
        let after = model.runs();
        if after != before {
            deliver.deliver(Arc::new(after));
        }
    });
    match connected {
        Ok(connected) => {
            tracing::info!(
                "following the workspaces through {:?}",
                connected.capabilities().protocol
            );
            *lock(manager) = Some(connected);
        }
        Err(ConnectError::Unsupported) => {
            tracing::info!("no workspace protocol offered: the workspace switcher is not shown");
        }
        Err(error) => {
            report::error("following the workspaces (the switcher shows none)", &error);
        }
    }
}

/// The switcher's change for a workspaces event; `None` for an event it
/// does not know.
fn change(event: Event) -> Option<Change<WorkspaceId, GroupId>> {
    Some(match event {
        Event::Added(workspace) | Event::Changed(workspace) => Change::Workspace(entry(workspace)),
        Event::Removed(id) => Change::Removed(id),
        Event::GroupChanged(group) => Change::Group {
            id: group.id,
            outputs: group.outputs.into_iter().collect(),
        },
        Event::GroupRemoved(id) => Change::GroupRemoved(id),
        Event::Ended(_) => Change::Ended,
        _ => return None,
    })
}

/// What the switcher uses of `workspace`.
fn entry(workspace: Workspace) -> Entry<WorkspaceId, GroupId> {
    Entry {
        id: workspace.id,
        active: workspace.state.contains(WorkspaceState::ACTIVE),
        hidden: workspace.state.contains(WorkspaceState::HIDDEN),
        group: workspace.group,
        coordinates: workspace.coordinates,
        activatable: workspace.actions.contains(Actions::ACTIVATE),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

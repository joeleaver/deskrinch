//! `ext_workspace_v1`, the standard protocol: workspace groups on outputs,
//! and workspaces in them, changes batched until the manager's `done`.

use std::collections::{BTreeSet, HashMap};

use deskrinch_wayland_client::Outputs;
use wayland_client::backend::ObjectId;
use wayland_client::globals::GlobalList;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::workspace::v1::client::ext_workspace_group_handle_v1::{
    self, ExtWorkspaceGroupHandleV1,
};
use wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1::{
    self, ExtWorkspaceHandleV1, State as ExtState, WorkspaceCapabilities,
};
use wayland_protocols::ext::workspace::v1::client::ext_workspace_manager_v1::{
    self, ExtWorkspaceManagerV1,
};

use super::{Backend, Handle, Shared, State};
use crate::capabilities::{Action, Actions, Capabilities, Protocol};
use crate::error::{ConnectError, EndReason};
use crate::model::Change;
use crate::workspace::{GroupId, WorkspaceId, WorkspaceState};

/// The manager's global.
pub(super) const GLOBAL: &str = "ext_workspace_manager_v1";
/// The version bound: the protocol's first, the only one there is.
const VERSION: u32 = 1;
/// The manager's first batch announces the groups and workspaces; a second
/// exchange makes sure their own first events have arrived.
pub(super) const INITIAL_ROUNDTRIPS: usize = 2;

/// What the protocol reports and carries; `outputs` is filled in from the
/// outputs offered.
pub(super) const CAPABILITIES: Capabilities = Capabilities {
    protocol: Protocol::ExtWorkspace,
    states: WorkspaceState::ACTIVE
        .union(WorkspaceState::URGENT)
        .union(WorkspaceState::HIDDEN),
    outputs: false,
    layout: true,
    actions: Actions::ACTIVATE,
};

/// Binds the manager.
pub(super) fn bind(
    globals: &GlobalList,
    qh: &QueueHandle<State>,
) -> Result<ExtWorkspaceManagerV1, ConnectError> {
    globals
        .bind(qh, VERSION..=VERSION, ())
        .map_err(|source| ConnectError::VersionTooOld {
            interface: GLOBAL,
            required: VERSION,
            source,
        })
}

/// Requests `action` on `handle`, committed through the manager.
pub(super) fn perform(
    handle: &ExtWorkspaceHandleV1,
    manager: Option<&ExtWorkspaceManagerV1>,
    action: Action,
) {
    match action {
        Action::Activate => handle.activate(),
    }
    if let Some(manager) = manager {
        manager.commit();
    }
}

/// The protocol objects' identities on the dispatch thread.
#[derive(Debug, Default)]
pub(super) struct Tracking {
    workspaces: HashMap<ObjectId, WorkspaceId>,
    /// Each group, and the outputs (by registry name) it is on.
    groups: HashMap<ObjectId, (GroupId, BTreeSet<u32>)>,
}

impl Tracking {
    /// Stages every group's outputs, by name.
    pub(super) fn stage_outputs(&self, shared: &Shared, outputs: &Outputs) {
        let mut inner = shared.lock();
        for (group, globals) in self.groups.values() {
            inner.list.stage_outputs(*group, outputs.names(globals));
        }
    }
}

/// The ext tracking of `state`, which this backend's objects only exist
/// beside.
fn tracking(backend: &mut Backend) -> Option<&mut Tracking> {
    match backend {
        Backend::Ext(tracking) => Some(tracking),
        Backend::Plasma(_) => None,
    }
}

/// The workspace state flags `state` sets; unknown bits are ignored.
fn workspace_state(state: WEnum<ExtState>) -> WorkspaceState {
    let bits = match state {
        WEnum::Value(state) => state.bits(),
        WEnum::Unknown(bits) => bits,
    };
    let known = ExtState::from_bits_truncate(bits);
    let mut flags = WorkspaceState::empty();
    flags.set(WorkspaceState::ACTIVE, known.contains(ExtState::Active));
    flags.set(WorkspaceState::URGENT, known.contains(ExtState::Urgent));
    flags.set(WorkspaceState::HIDDEN, known.contains(ExtState::Hidden));
    flags
}

/// The actions `capabilities` allows that this crate carries.
fn workspace_actions(capabilities: WEnum<WorkspaceCapabilities>) -> Actions {
    let bits = match capabilities {
        WEnum::Value(capabilities) => capabilities.bits(),
        WEnum::Unknown(bits) => bits,
    };
    let known = WorkspaceCapabilities::from_bits_truncate(bits);
    let mut actions = Actions::empty();
    actions.set(
        Actions::ACTIVATE,
        known.contains(WorkspaceCapabilities::Activate),
    );
    actions
}

/// Coordinates sent as an array of `u32`s in the host's byte order; `None`
/// when the array is not a whole number of them.
fn coordinates(bytes: &[u8]) -> Option<Vec<u32>> {
    let (chunks, rest) = bytes.as_chunks::<4>();
    rest.is_empty().then(|| {
        chunks
            .iter()
            .map(|chunk| u32::from_ne_bytes(*chunk))
            .collect()
    })
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                let group = state.shared.lock().list.add_group();
                match (group, tracking(&mut state.backend)) {
                    (Some(group), Some(tracking)) => {
                        tracking
                            .groups
                            .insert(workspace_group.id(), (group, BTreeSet::new()));
                    }
                    // Beyond the bound, or not this backend: not listed.
                    _ => workspace_group.destroy(),
                }
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                let id = state.shared.lock().list.add();
                match (id, tracking(&mut state.backend)) {
                    (Some(id), Some(tracking)) => {
                        tracking.workspaces.insert(workspace.id(), id);
                        let mut inner = state.shared.lock();
                        inner.handles.insert(id, Handle::Ext(workspace));
                    }
                    _ => workspace.destroy(),
                }
            }
            ext_workspace_manager_v1::Event::Done => state.commit(),
            ext_workspace_manager_v1::Event::Finished => state.end(EndReason::Finished),
            _ => {}
        }
    }

    wayland_client::event_created_child!(State, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

/// The group `proxy` came onto (`entered`) or left `output`; its outputs'
/// names are staged.
fn output_moved(
    state: &mut State,
    proxy: &ExtWorkspaceGroupHandleV1,
    output: &wayland_client::protocol::wl_output::WlOutput,
    entered: bool,
) {
    let Some(global) = Outputs::global_of(output) else {
        return;
    };
    let Some(tracking) = tracking(&mut state.backend) else {
        return;
    };
    let Some((group, globals)) = tracking.groups.get_mut(&proxy.id()) else {
        return;
    };
    if entered {
        globals.insert(global);
    } else {
        globals.remove(&global);
    }
    let (group, globals) = (*group, globals.clone());
    let names = state.outputs.names(&globals);
    state.shared.lock().list.stage_outputs(group, names);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_group_handle_v1::Event::OutputEnter { output } => {
                output_moved(state, proxy, &output, true);
                return;
            }
            ext_workspace_group_handle_v1::Event::OutputLeave { output } => {
                output_moved(state, proxy, &output, false);
                return;
            }
            _ => {}
        }
        let Some(tracking) = tracking(&mut state.backend) else {
            return;
        };
        let Some(&(group, _)) = tracking.groups.get(&proxy.id()) else {
            return;
        };
        match event {
            ext_workspace_group_handle_v1::Event::WorkspaceEnter { workspace } => {
                if let Some(&id) = tracking.workspaces.get(&workspace.id()) {
                    state
                        .shared
                        .lock()
                        .list
                        .stage(id, Change::Group(Some(group)));
                }
            }
            ext_workspace_group_handle_v1::Event::WorkspaceLeave { workspace } => {
                if let Some(&id) = tracking.workspaces.get(&workspace.id()) {
                    state.shared.lock().list.stage(id, Change::Group(None));
                }
            }
            ext_workspace_group_handle_v1::Event::Removed => {
                tracking.groups.remove(&proxy.id());
                state.shared.lock().list.remove_group(group);
                proxy.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(tracking) = tracking(&mut state.backend) else {
            return;
        };
        let Some(&id) = tracking.workspaces.get(&proxy.id()) else {
            return;
        };
        let change = match event {
            ext_workspace_handle_v1::Event::Name { name } => Change::Name(name),
            ext_workspace_handle_v1::Event::State { state } => {
                Change::State(workspace_state(state))
            }
            ext_workspace_handle_v1::Event::Coordinates { coordinates: bytes } => {
                Change::Coordinates(coordinates(&bytes).unwrap_or_default())
            }
            ext_workspace_handle_v1::Event::Capabilities { capabilities } => {
                Change::Actions(workspace_actions(capabilities))
            }
            ext_workspace_handle_v1::Event::Removed => {
                tracking.workspaces.remove(&proxy.id());
                let mut inner = state.shared.lock();
                inner.handles.remove(&id);
                inner.list.remove(id);
                drop(inner);
                proxy.destroy();
                return;
            }
            _ => return,
        };
        state.shared.lock().list.stage(id, change);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_and_capabilities_keep_only_known_bits() {
        let all = WEnum::Unknown(0xff);
        assert_eq!(
            workspace_state(all),
            WorkspaceState::ACTIVE | WorkspaceState::URGENT | WorkspaceState::HIDDEN
        );
        assert_eq!(
            workspace_state(WEnum::Value(ExtState::Active)),
            WorkspaceState::ACTIVE
        );
        assert_eq!(workspace_actions(WEnum::Unknown(0xff)), Actions::ACTIVATE);
        assert_eq!(
            workspace_actions(WEnum::Value(WorkspaceCapabilities::Remove)),
            Actions::empty()
        );
    }

    #[test]
    fn coordinates_are_whole_u32s() {
        let bytes: Vec<u8> = [2_u32, 7].iter().flat_map(|n| n.to_ne_bytes()).collect();
        assert_eq!(coordinates(&bytes), Some(vec![2, 7]));
        assert_eq!(coordinates(&bytes[..5]), None);
        assert_eq!(coordinates(&[]), Some(vec![]));
    }
}

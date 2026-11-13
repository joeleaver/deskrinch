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

use super::{Backend, Handle, State};
use crate::capabilities::{Action, Actions, Capabilities, Protocol};
use crate::error::{ConnectError, EndReason};
use crate::model::{Change, WorkspaceList};
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

/// A group's outputs, by registry name: as of the manager's last `done`,
/// and as `output_enter` and `output_leave` have staged them since, in the
/// batch still open.
#[derive(Debug, Default)]
struct GroupOutputs {
    committed: BTreeSet<u32>,
    staged: BTreeSet<u32>,
}

/// The protocol objects' identities on the client's thread.
#[derive(Debug, Default)]
pub(super) struct Tracking {
    workspaces: HashMap<ObjectId, WorkspaceId>,
    /// Each group, and the outputs it is on.
    groups: HashMap<ObjectId, (GroupId, GroupOutputs)>,
}

impl Tracking {
    /// Stages every group's outputs as of the manager's last `done`, by
    /// `names`: what an output's name changing or the output going away
    /// changes between batches. What the open batch has staged is not
    /// named until its `done`.
    pub(super) fn stage_outputs(
        &self,
        list: &mut WorkspaceList,
        names: impl Fn(&BTreeSet<u32>) -> BTreeSet<String>,
    ) {
        for (group, outputs) in self.groups.values() {
            list.stage_outputs(*group, names(&outputs.committed));
        }
    }

    /// The manager's `done`: the outputs staged in the batch become the
    /// groups' outputs, and are staged by `names` for its commit.
    fn commit_outputs(
        &mut self,
        list: &mut WorkspaceList,
        names: impl Fn(&BTreeSet<u32>) -> BTreeSet<String>,
    ) {
        for (_, outputs) in self.groups.values_mut() {
            outputs.committed.clone_from(&outputs.staged);
        }
        self.stage_outputs(list, names);
    }

    /// Group `group` came onto (`entered`) or left the output with registry
    /// name `global`, in the open batch.
    fn output_moved(&mut self, group: &ObjectId, global: u32, entered: bool) {
        let Some((_, outputs)) = self.groups.get_mut(group) else {
            return;
        };
        if entered {
            outputs.staged.insert(global);
        } else {
            outputs.staged.remove(&global);
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
                            .insert(workspace_group.id(), (group, GroupOutputs::default()));
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
            ext_workspace_manager_v1::Event::Done => done(state),
            ext_workspace_manager_v1::Event::Finished => state.end(EndReason::Finished),
            _ => {}
        }
    }

    wayland_client::event_created_child!(State, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

/// The group `proxy` came onto (`entered`) or left `output`, in the open
/// batch. Coming onto an output whose global has gone is ignored: it is not
/// listed again.
fn output_moved(
    state: &mut State,
    proxy: &ExtWorkspaceGroupHandleV1,
    output: &wayland_client::protocol::wl_output::WlOutput,
    entered: bool,
) {
    let Some(global) = Outputs::global_of(output) else {
        return;
    };
    if entered && !state.outputs.contains(global) {
        return;
    }
    if let Some(tracking) = tracking(&mut state.backend) {
        tracking.output_moved(&proxy.id(), global, entered);
    }
}

/// The manager's `done`: the groups' outputs staged in the batch are
/// theirs now, and the batch is delivered.
fn done(state: &mut State) {
    if let Backend::Ext(tracking) = &mut state.backend {
        let mut inner = state.shared.lock();
        tracking.commit_outputs(&mut inner.list, |globals| state.outputs.names(globals));
    }
    state.commit();
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
    use crate::workspace::Event;

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

    /// Names outputs as `HEADLESS-<registry name>`, or `renamed-<registry
    /// name>` when `renamed`.
    fn named(renamed: bool) -> impl Fn(&BTreeSet<u32>) -> BTreeSet<String> {
        move |globals| {
            let prefix = if renamed { "renamed" } else { "HEADLESS" };
            globals
                .iter()
                .map(|global| format!("{prefix}-{global}"))
                .collect()
        }
    }

    fn outputs_of(events: &[Event]) -> Vec<BTreeSet<String>> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::GroupChanged(group) => Some(group.outputs.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_output_entered_in_an_open_batch_waits_for_its_done() {
        let mut list = WorkspaceList::default();
        let mut tracking = Tracking::default();
        let object = ObjectId::null();
        let group = list.add_group().unwrap();
        tracking
            .groups
            .insert(object.clone(), (group, GroupOutputs::default()));
        tracking.output_moved(&object, 1, true);
        tracking.commit_outputs(&mut list, named(false));
        let first = list.commit();
        assert_eq!(outputs_of(&first), [BTreeSet::from(["HEADLESS-1".into()])]);
        // A batch opens: the group comes onto output 2 and leaves 1, then
        // the outputs are renamed before the batch's `done`.
        tracking.output_moved(&object, 2, true);
        tracking.output_moved(&object, 1, false);
        tracking.stage_outputs(&mut list, named(true));
        let renamed = list.commit_groups();
        assert_eq!(
            outputs_of(&renamed),
            [BTreeSet::from(["renamed-1".into()])],
            "the outputs as of the last done, renamed"
        );
        tracking.commit_outputs(&mut list, named(true));
        let done = list.commit();
        assert_eq!(outputs_of(&done), [BTreeSet::from(["renamed-2".into()])]);
    }

    #[test]
    fn coordinates_are_whole_u32s() {
        let bytes: Vec<u8> = [2_u32, 7].iter().flat_map(|n| n.to_ne_bytes()).collect();
        assert_eq!(coordinates(&bytes), Some(vec![2, 7]));
        assert_eq!(coordinates(&bytes[..5]), None);
        assert_eq!(coordinates(&[]), Some(vec![]));
    }
}

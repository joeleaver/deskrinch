//! `org_kde_plasma_virtual_desktop_management`, KWin's: desktops shared by
//! every output, in an order and a number of rows.
//!
//! Bound at version 2, the newest the Rust bindings
//! (`wayland-protocols-plasma`) carry; KWin 6.6 offers version 3, which
//! adds the `request_move_virtual_desktop` request (reordering desktops),
//! not needed for following and switching. Whoever needs it looks there,
//! with bindings that carry it.
//!
//! # Where KWin ends a batch
//!
//! As KWin 6.6.6 sends them (a `WAYLAND_DEBUG` trace of the nested session):
//!
//! - A switch, asked for by `request_activate` on the desktop switched to
//!   or made by KWin itself (its D-Bus `current`): `deactivated` on the
//!   desktop left, `activated` on the one switched to, then the manager's
//!   `done`. The two desktops' events are
//!   staged; the manager's `done` delivers them as one batch.
//! - A desktop created: the manager's `desktop_created` and its `done`.
//!   Once the client asks for the desktop with `get_virtual_desktop`, the
//!   desktop sends its `desktop_id`, `name` (and `activated` if it is the
//!   current one), then its own `done`. The desktop is not delivered until
//!   that `done`, which delivers it.
//! - A desktop removed (by KWin): `removed` on the desktop, the manager's
//!   `desktop_removed`, then the manager's `done`. Either of the first two
//!   stages the removal; the manager's `done` delivers it.
//!
//! So the manager's `done` ends every batch but a new desktop's
//! description, which the desktop's own `done` ends. Nothing incidental
//! (an output's event, say) is needed to deliver a batch.
//!
//! # Releasing
//!
//! Neither interface has a destructor or a `stop`:
//! `org_kde_plasma_virtual_desktop_management` and every
//! `org_kde_plasma_virtual_desktop` stay alive, and keep sending (on every
//! switch, `deactivated`, `activated` and the manager's `done`), for as
//! long as the connection lasts. A released state forgets them and is
//! never dropped: its events are dispatched and thrown away.

use std::collections::HashMap;

use deskrinch_wayland_client::Outputs;
use wayland_client::globals::GlobalList;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_plasma::plasma_virtual_desktop::client::org_kde_plasma_virtual_desktop::{
    self, OrgKdePlasmaVirtualDesktop,
};
use wayland_protocols_plasma::plasma_virtual_desktop::client::org_kde_plasma_virtual_desktop_management::{
    self, OrgKdePlasmaVirtualDesktopManagement,
};

use super::{Backend, Handle, Shared, State};
use crate::capabilities::{Action, Actions, Capabilities, Protocol};
use crate::error::ConnectError;
use crate::model::{Change, WorkspaceList};
use crate::workspace::{GroupId, WorkspaceId, WorkspaceState};

/// The manager's global.
pub(super) const GLOBAL: &str = "org_kde_plasma_virtual_desktop_management";
/// The oldest version bound: the first.
const MIN_VERSION: u32 = 1;
/// The newest version the bindings carry (`rows` arrived in it).
const MAX_VERSION: u32 = 2;
/// The manager announces the desktops; a second exchange brings each
/// desktop's own first events.
pub(super) const INITIAL_ROUNDTRIPS: usize = 2;
/// The longest desktop id kept; a desktop with a longer one is not listed.
const MAX_ID_BYTES: usize = 256;
/// The most rows a layout may have.
const MAX_ROWS: u32 = 64;

/// What the protocol reports and carries; `outputs` is filled in from the
/// outputs offered.
pub(super) const CAPABILITIES: Capabilities = Capabilities {
    protocol: Protocol::PlasmaVirtualDesktop,
    states: WorkspaceState::ACTIVE,
    outputs: false,
    layout: true,
    actions: Actions::ACTIVATE,
};

/// Binds the manager and lists the one group KWin's desktops share.
pub(super) fn bind(
    globals: &GlobalList,
    shared: &Shared,
    qh: &QueueHandle<State>,
) -> Result<Tracking, ConnectError> {
    let manager: OrgKdePlasmaVirtualDesktopManagement = globals
        .bind(qh, MIN_VERSION..=MAX_VERSION, ())
        .map_err(|source| ConnectError::VersionTooOld {
            interface: GLOBAL,
            required: MIN_VERSION,
            source,
        })?;
    let group = shared.lock().list.add_group();
    Ok(Tracking {
        manager,
        ids: HashMap::new(),
        rows: 1,
        group,
    })
}

/// Requests `action` on `desktop`.
pub(super) fn perform(desktop: &OrgKdePlasmaVirtualDesktop, action: Action) {
    match action {
        Action::Activate => desktop.request_activate(),
    }
}

/// The desktops' identities on the client's thread.
#[derive(Debug)]
pub(super) struct Tracking {
    manager: OrgKdePlasmaVirtualDesktopManagement,
    /// Each desktop, by KWin's id.
    ids: HashMap<String, WorkspaceId>,
    /// The rows KWin lays the desktops out in.
    rows: u32,
    /// The one group, on every output.
    group: Option<GroupId>,
}

impl Tracking {
    /// Stages the group's outputs: every output.
    pub(super) fn stage_outputs(&self, list: &mut WorkspaceList, outputs: &Outputs) {
        if let Some(group) = self.group {
            list.stage_outputs(group, outputs.all_names());
        }
    }
}

/// The plasma tracking of `backend`, which this backend's objects only
/// exist beside.
fn tracking(backend: &mut Backend) -> Option<&mut Tracking> {
    match backend {
        Backend::Plasma(tracking) => Some(tracking),
        Backend::Ext(_) | Backend::Released(_) => None,
    }
}

/// Each position's `[row, column]` for `count` desktops in `rows` rows,
/// filled row by row as KWin lays them out.
fn grid(count: usize, rows: u32) -> impl Iterator<Item = Vec<u32>> {
    let rows = usize::try_from(rows.max(1)).unwrap_or(1).min(count.max(1));
    let columns = count.div_ceil(rows).max(1);
    (0..count).map(move |index| {
        let at = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        vec![at(index / columns), at(index % columns)]
    })
}

/// Stages every desktop's coordinates from the order and rows, then
/// delivers the batch.
fn commit(state: &mut State) {
    let rows = tracking(&mut state.backend).map_or(1, |tracking| tracking.rows);
    {
        let mut inner = state.shared.lock();
        let order = inner.list.order().to_vec();
        for (id, coordinates) in order.iter().zip(grid(order.len(), rows)) {
            inner.list.stage(*id, Change::Coordinates(coordinates));
        }
    }
    state.commit();
}

/// Forgets desktop `id`; its removal is delivered at the next commit.
fn forget(state: &mut State, id: WorkspaceId) {
    if let Some(tracking) = tracking(&mut state.backend) {
        tracking.ids.retain(|_, listed| *listed != id);
    }
    let mut inner = state.shared.lock();
    inner.handles.remove(&id);
    inner.list.remove(id);
}

impl Dispatch<OrgKdePlasmaVirtualDesktopManagement, ()> for State {
    fn event(
        state: &mut Self,
        _: &OrgKdePlasmaVirtualDesktopManagement,
        event: org_kde_plasma_virtual_desktop_management::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            org_kde_plasma_virtual_desktop_management::Event::DesktopCreated {
                desktop_id,
                position,
            } => {
                if desktop_id.len() > MAX_ID_BYTES {
                    return;
                }
                let Some(tracking) = tracking(&mut state.backend) else {
                    return;
                };
                if tracking.ids.contains_key(&desktop_id) {
                    return;
                }
                let mut inner = state.shared.lock();
                // Not delivered until the desktop's own `done`: KWin sends
                // its name after the manager's.
                let Some(id) = inner.list.add_unready() else {
                    return;
                };
                let desktop = tracking
                    .manager
                    .get_virtual_desktop(desktop_id.clone(), qh, id);
                tracking.ids.insert(desktop_id, id);
                inner
                    .list
                    .move_to(id, usize::try_from(position).unwrap_or(usize::MAX));
                inner.list.stage(id, Change::Group(tracking.group));
                inner.list.stage(id, Change::Actions(Actions::ACTIVATE));
                inner.handles.insert(id, Handle::Plasma(desktop));
            }
            org_kde_plasma_virtual_desktop_management::Event::DesktopRemoved { desktop_id } => {
                let id = tracking(&mut state.backend)
                    .and_then(|tracking| tracking.ids.get(&desktop_id).copied());
                if let Some(id) = id {
                    forget(state, id);
                }
            }
            org_kde_plasma_virtual_desktop_management::Event::Rows { rows } => {
                if let Some(tracking) = tracking(&mut state.backend) {
                    tracking.rows = rows.clamp(1, MAX_ROWS);
                }
            }
            org_kde_plasma_virtual_desktop_management::Event::Done => commit(state),
            _ => {}
        }
    }
}

impl Dispatch<OrgKdePlasmaVirtualDesktop, WorkspaceId> for State {
    fn event(
        state: &mut Self,
        _: &OrgKdePlasmaVirtualDesktop,
        event: org_kde_plasma_virtual_desktop::Event,
        &id: &WorkspaceId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Released: a desktop has no destructor, so its events keep coming
        // and are dropped here.
        if tracking(&mut state.backend).is_none() {
            return;
        }
        let change = match event {
            org_kde_plasma_virtual_desktop::Event::Name { name } => Change::Name(name),
            org_kde_plasma_virtual_desktop::Event::Activated => {
                Change::State(WorkspaceState::ACTIVE)
            }
            org_kde_plasma_virtual_desktop::Event::Deactivated => {
                Change::State(WorkspaceState::empty())
            }
            org_kde_plasma_virtual_desktop::Event::Done => {
                state.shared.lock().list.ready(id);
                commit(state);
                return;
            }
            // Staged: the manager's `done` that follows ends the batch, with
            // whatever else the removal changed.
            org_kde_plasma_virtual_desktop::Event::Removed => {
                forget(state, id);
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

    fn cells(count: usize, rows: u32) -> Vec<Vec<u32>> {
        grid(count, rows).collect()
    }

    #[test]
    fn desktops_fill_the_rows_row_by_row() {
        assert_eq!(cells(4, 1), [[0, 0], [0, 1], [0, 2], [0, 3]]);
        assert_eq!(cells(4, 2), [[0, 0], [0, 1], [1, 0], [1, 1]]);
        assert_eq!(cells(5, 2), [[0, 0], [0, 1], [0, 2], [1, 0], [1, 1]]);
    }

    #[test]
    fn rows_beyond_the_desktops_or_none_at_all_are_tamed() {
        assert_eq!(cells(2, 9), [[0, 0], [1, 0]]);
        assert_eq!(cells(2, 0), [[0, 0], [0, 1]]);
        assert_eq!(cells(0, 3), Vec::<Vec<u32>>::new());
    }
}

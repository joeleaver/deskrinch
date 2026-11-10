//! The KWin backend: `org_kde_plasma_window_management`.
//!
//! The manager announces each window by uuid (`window_with_uuid`); this
//! backend asks for the window object, which then reports its properties and
//! `initial_state`. After `initial_state` every property event stands alone,
//! so it is announced at once. `unmapped` ends the window.

use wayland_client::globals::GlobalList;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_plasma::plasma_window_management::client::org_kde_plasma_window::{
    self, OrgKdePlasmaWindow,
};
use wayland_protocols_plasma::plasma_window_management::client::org_kde_plasma_window_management::{
    self, OrgKdePlasmaWindowManagement, State as PlasmaState,
};

use super::{State, WindowHandle};
use crate::capabilities::{Action, Actions, Capabilities, Protocol};
use crate::error::ConnectError;
use crate::layout::Rect;
use crate::model::Change;
use crate::window::{WindowId, WindowState};

/// The manager's global interface.
pub(super) const GLOBAL: &str = "org_kde_plasma_window_management";

/// `window_with_uuid` and `get_window_by_uuid` arrived in version 13.
const MIN_VERSION: u32 = 13;
/// The newest version the bindings know.
const MAX_VERSION: u32 = 18;

/// Exchanges with the compositor that load the initial list: the first
/// announces the windows, the second brings each one's initial state.
pub(super) const INITIAL_ROUNDTRIPS: usize = 2;

/// Binds the manager.
pub(super) fn bind(
    globals: &GlobalList,
    qh: &QueueHandle<State>,
) -> Result<(OrgKdePlasmaWindowManagement, Capabilities), ConnectError> {
    let manager = globals
        .bind(qh, MIN_VERSION..=MAX_VERSION, ())
        .map_err(|source| ConnectError::VersionTooOld {
            interface: GLOBAL,
            required: MIN_VERSION,
            source,
        })?;
    let capabilities = Capabilities {
        protocol: Protocol::PlasmaWindowManagement,
        states: WindowState::all(),
        // Worked out from geometry; settled by `backend::connect`, which
        // knows whether the output layout is described.
        outputs: false,
        // `parent_window` is older than `MIN_VERSION`.
        parent: true,
        actions: Actions::all(),
    };
    Ok((manager, capabilities))
}

/// The protocol's `state` enum is a set of bit values, sent as a plain
/// `uint` bitfield; these are the bits this crate models.
const MODELLED: [(PlasmaState, WindowState); 6] = [
    (PlasmaState::Active, WindowState::ACTIVE),
    (PlasmaState::Minimized, WindowState::MINIMIZED),
    (PlasmaState::Maximized, WindowState::MAXIMIZED),
    (PlasmaState::Fullscreen, WindowState::FULLSCREEN),
    (
        PlasmaState::DemandsAttention,
        WindowState::DEMANDS_ATTENTION,
    ),
    (PlasmaState::Skiptaskbar, WindowState::SKIP_TASKBAR),
];

/// A protocol state value as its bit.
fn bit(state: PlasmaState) -> u32 {
    u32::from(state)
}

/// Requests `action` on `window`.
pub(super) fn perform(window: &OrgKdePlasmaWindow, action: Action) {
    // `set_state(flags, state)`: which bits to change, and their new values.
    let (flags, state) = match action {
        Action::Activate => (bit(PlasmaState::Active), bit(PlasmaState::Active)),
        Action::Minimize => (bit(PlasmaState::Minimized), bit(PlasmaState::Minimized)),
        Action::Restore => (bit(PlasmaState::Minimized), 0),
        Action::Close => return window.close(),
    };
    window.set_state(flags, state);
}

/// The crate's state flags for the protocol's `state` bitfield. Bits this
/// crate does not model (and bits newer than the bindings) are dropped.
fn window_state(flags: u32) -> WindowState {
    MODELLED
        .into_iter()
        .filter(|&(plasma, _)| flags & bit(plasma) != 0)
        .fold(WindowState::empty(), |state, (_, ours)| state | ours)
}

impl Dispatch<OrgKdePlasmaWindowManagement, ()> for State {
    fn event(
        state: &mut Self,
        manager: &OrgKdePlasmaWindowManagement,
        event: org_kde_plasma_window_management::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        // Stacking order, show-desktop and the deprecated id-only `window`
        // event are not modelled.
        if let org_kde_plasma_window_management::Event::WindowWithUuid { uuid, .. } = event {
            let id = WindowId::next();
            let window = manager.get_window_by_uuid(uuid, qh, id);
            state.track(id, WindowHandle::Plasma(window));
        }
    }
}

impl Dispatch<OrgKdePlasmaWindow, WindowId> for State {
    fn event(
        state: &mut Self,
        window: &OrgKdePlasmaWindow,
        event: org_kde_plasma_window::Event,
        &id: &WindowId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let change = match event {
            org_kde_plasma_window::Event::TitleChanged { title } => Change::Title(title),
            org_kde_plasma_window::Event::AppIdChanged { app_id } => Change::AppId(app_id),
            org_kde_plasma_window::Event::StateChanged { flags } => {
                Change::State(window_state(flags))
            }
            org_kde_plasma_window::Event::Geometry {
                x,
                y,
                width,
                height,
            } => {
                let geometry = Rect {
                    x,
                    y,
                    width: i32::try_from(width).unwrap_or(i32::MAX),
                    height: i32::try_from(height).unwrap_or(i32::MAX),
                };
                state.moved(id, geometry);
                return;
            }
            org_kde_plasma_window::Event::ParentWindow { parent } => {
                Change::Parent(parent.and_then(|parent| parent.data::<WindowId>().copied()))
            }
            org_kde_plasma_window::Event::InitialState => {
                state.update(|inner| inner.list.commit(id));
                return;
            }
            org_kde_plasma_window::Event::Unmapped => {
                state.untrack(id);
                window.destroy();
                return;
            }
            // Icons, virtual desktops, activities, pid and the rest are not
            // modelled.
            _ => return,
        };
        state.update(|inner| inner.list.apply_live(id, change));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modelled_flags_map_one_to_one() {
        let all = MODELLED
            .into_iter()
            .fold(0, |flags, (plasma, _)| flags | bit(plasma));
        assert_eq!(window_state(all), WindowState::all());
        assert_eq!(
            window_state(bit(PlasmaState::Minimized)),
            WindowState::MINIMIZED
        );
    }

    #[test]
    fn unmodelled_and_unknown_flags_are_dropped() {
        let flags = bit(PlasmaState::Active)
            | bit(PlasmaState::KeepAbove)
            | bit(PlasmaState::Closeable)
            | 1 << 31;
        assert_eq!(window_state(flags), WindowState::ACTIVE);
    }
}

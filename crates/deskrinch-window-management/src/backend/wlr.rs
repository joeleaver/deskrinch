//! The wlroots backend: `zwlr_foreign_toplevel_management_v1`.
//!
//! The manager creates a handle per toplevel; the handle reports properties
//! and then `done`, which marks a consistent state, so changes are staged
//! until `done`. `closed` ends the toplevel.

use wayland_client::globals::GlobalList;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::{
    self, State as WlrState, ZwlrForeignToplevelHandleV1,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::{
    self, ZwlrForeignToplevelManagerV1,
};

use super::{State, WindowHandle};
use crate::capabilities::{Action, Actions, Capabilities, Protocol};
use crate::error::{ActionError, ConnectError, EndReason};
use crate::model::Change;
use crate::window::{WindowId, WindowState};

/// The manager's global interface.
pub(super) const GLOBAL: &str = "zwlr_foreign_toplevel_manager_v1";

/// Every version is usable; version 2 adds the fullscreen state.
const MIN_VERSION: u32 = 1;
/// The newest version the bindings know.
const MAX_VERSION: u32 = 3;
/// The version that reports fullscreen.
const FULLSCREEN_SINCE: u32 = 2;

/// The protocol sends a new toplevel's initial state right after announcing
/// it, so one exchange loads the list.
pub(super) const INITIAL_ROUNDTRIPS: usize = 1;

/// Binds the manager.
pub(super) fn bind(
    globals: &GlobalList,
    qh: &QueueHandle<State>,
) -> Result<(ZwlrForeignToplevelManagerV1, Capabilities), ConnectError> {
    let manager: ZwlrForeignToplevelManagerV1 = globals
        .bind(qh, MIN_VERSION..=MAX_VERSION, ())
        .map_err(|source| ConnectError::VersionTooOld {
            interface: GLOBAL,
            required: MIN_VERSION,
            source,
        })?;
    let mut states = WindowState::ACTIVE | WindowState::MINIMIZED | WindowState::MAXIMIZED;
    if manager.version() >= FULLSCREEN_SINCE {
        states |= WindowState::FULLSCREEN;
    }
    let capabilities = Capabilities {
        protocol: Protocol::WlrForeignToplevel,
        states,
        outputs: true,
        actions: Actions::all(),
    };
    Ok((manager, capabilities))
}

/// Requests `action` on `handle`; activating needs a seat.
pub(super) fn perform(
    handle: &ZwlrForeignToplevelHandleV1,
    action: Action,
    seat: Option<&WlSeat>,
) -> Result<(), ActionError> {
    match action {
        Action::Activate => handle.activate(seat.ok_or(ActionError::NoSeat)?),
        Action::Minimize => handle.set_minimized(),
        Action::Restore => handle.unset_minimized(),
        Action::Close => handle.close(),
    }
    Ok(())
}

/// The crate's state flags for the protocol's `state` array of `u32`s, in
/// native byte order. Values this crate does not know are dropped.
fn window_state(array: &[u8]) -> WindowState {
    let (values, _fragment) = array.as_chunks::<4>();
    values
        .iter()
        .map(|&bytes| u32::from_ne_bytes(bytes))
        .filter_map(|value| match WlrState::try_from(value) {
            Ok(WlrState::Activated) => Some(WindowState::ACTIVE),
            Ok(WlrState::Minimized) => Some(WindowState::MINIMIZED),
            Ok(WlrState::Maximized) => Some(WindowState::MAXIMIZED),
            Ok(WlrState::Fullscreen) => Some(WindowState::FULLSCREEN),
            _ => None,
        })
        .fold(WindowState::empty(), |state, flag| state | flag)
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                if let Some(&id) = toplevel.data::<WindowId>() {
                    state.track(id, WindowHandle::Wlr(toplevel));
                }
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => state.end(EndReason::Finished),
            _ => {}
        }
    }

    event_created_child!(State, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, WindowId::next()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, WindowId> for State {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        &id: &WindowId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let change = match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => Change::Title(title),
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => Change::AppId(app_id),
            zwlr_foreign_toplevel_handle_v1::Event::State { state: array } => {
                Change::State(window_state(&array))
            }
            zwlr_foreign_toplevel_handle_v1::Event::OutputEnter { output } => {
                let Some(name) = state.output_name(&output) else {
                    return;
                };
                Change::OutputEnter(name)
            }
            zwlr_foreign_toplevel_handle_v1::Event::OutputLeave { output } => {
                let Some(name) = state.output_name(&output) else {
                    return;
                };
                Change::OutputLeave(name)
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                state.update(|inner| inner.list.commit(id));
                return;
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.untrack(id);
                handle.destroy();
                return;
            }
            // `parent` is not modelled.
            _ => return,
        };
        state.update(|inner| {
            inner.list.stage(id, change);
            None
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn array(values: &[u32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect()
    }

    #[test]
    fn reads_every_known_state() {
        let all = array(&[0, 1, 2, 3]);
        assert_eq!(
            window_state(&all),
            WindowState::MAXIMIZED
                | WindowState::MINIMIZED
                | WindowState::ACTIVE
                | WindowState::FULLSCREEN
        );
    }

    #[test]
    fn an_empty_array_is_no_state() {
        assert_eq!(window_state(&[]), WindowState::empty());
    }

    #[test]
    fn unknown_values_and_a_trailing_fragment_are_dropped() {
        let mut bytes = array(&[2, 99]);
        bytes.push(1);
        assert_eq!(window_state(&bytes), WindowState::ACTIVE);
    }
}

//! The protocol side: the state attached to the client's connection, the
//! globals both backends share (outputs, a seat), and the choice of
//! backend.
//!
//! [`State`] is what the client's thread dispatches into. What the handle
//! also needs (the list, the protocol objects that actions are sent on) is
//! in [`Shared`], behind a mutex that is never held while the subscriber
//! runs.

mod plasma;
mod wlr;

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use deskrinch_wayland_client::{
    AttachError, Attachment, Client, Detached, EndCause, Ended, OutputGlobal, Outputs,
    OutputsHandler, Setup, layout_available, names_available,
};
use wayland_client::globals::{GlobalList, GlobalListContents};
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_plasma::plasma_window_management::client::org_kde_plasma_window::OrgKdePlasmaWindow;
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1;

use crate::capabilities::{Action, Capabilities, Protocol};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::ZxdgOutputV1;

use crate::error::{ActionError, ConnectError, EndReason};
use crate::layout::{self, Rect};
use crate::model::{Change, WindowList};
use crate::window::{Event, WindowId};

/// The globals a backend exists for, in order of preference.
pub(crate) const SUPPORTED_GLOBALS: [&str; 2] = [wlr::GLOBAL, plasma::GLOBAL];

/// Activation only names the seat; any version will do.
const SEAT_VERSION: u32 = 1;

/// What receives [`Event`]s.
pub(crate) type Subscriber = Box<dyn Fn(Event) + Send>;

/// The protocol object a window's actions are sent on.
#[derive(Debug)]
pub(crate) enum WindowHandle {
    Plasma(OrgKdePlasmaWindow),
    Wlr(ZwlrForeignToplevelHandleV1),
}

/// What the dispatch thread and the handle share.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    inner: Mutex<Inner>,
}

impl Shared {
    /// Locks the shared state. Nothing panics while holding the lock (the
    /// subscriber runs outside it), so a poisoned lock still holds
    /// consistent state and is used as is.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The state behind [`Shared`]'s lock.
#[derive(Debug, Default)]
pub(crate) struct Inner {
    pub(crate) list: WindowList,
    handles: HashMap<WindowId, WindowHandle>,
    seat: Option<WlSeat>,
    ended: bool,
}

impl Inner {
    /// Sends `action` for `id`; the caller flushes.
    pub(crate) fn request(
        &self,
        capabilities: Capabilities,
        id: WindowId,
        action: Action,
    ) -> Result<(), ActionError> {
        if self.ended {
            return Err(ActionError::Ended);
        }
        if !capabilities.supports(action) {
            return Err(ActionError::Unsupported(action));
        }
        match self
            .handles
            .get(&id)
            .ok_or(ActionError::UnknownWindow(id))?
        {
            WindowHandle::Plasma(window) => {
                plasma::perform(window, action);
                Ok(())
            }
            WindowHandle::Wlr(handle) => wlr::perform(handle, action, self.seat.as_ref()),
        }
    }
}

/// Where a window is, as its protocol reports it.
#[derive(Debug)]
enum Placement {
    /// On these outputs, by registry name.
    Outputs(BTreeSet<u32>),
    /// At this rectangle in the global layout.
    Geometry(Rect),
}

/// The state attached to the client's connection.
pub(crate) struct State {
    shared: Arc<Shared>,
    subscriber: Subscriber,
    /// Set once the handle is dropped: the subscriber hears nothing more.
    detached: Detached,
    outputs: Outputs,
    /// Where each window is, for those whose protocol has said.
    placements: HashMap<WindowId, Placement>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("shared", &self.shared)
            .field("outputs", &self.outputs)
            .field("placements", &self.placements)
            .finish_non_exhaustive()
    }
}

impl State {
    /// Runs `change` under the lock, then delivers what it produced.
    fn update<E: IntoIterator<Item = Event>>(&mut self, change: impl FnOnce(&mut Inner) -> E) {
        let events: Vec<_> = change(&mut self.shared.lock()).into_iter().collect();
        for event in events {
            self.deliver(event);
        }
    }

    /// Tells the subscriber, unless the handle has been dropped.
    fn deliver(&self, event: Event) {
        if !self.detached.is_set() {
            (self.subscriber)(event);
        }
    }

    /// Starts tracking a window the protocol created.
    fn track(&mut self, id: WindowId, handle: WindowHandle) {
        let mut inner = self.shared.lock();
        inner.list.insert(id);
        inner.handles.insert(id, handle);
    }

    /// Stops tracking a window the protocol ended.
    fn untrack(&mut self, id: WindowId) {
        self.placements.remove(&id);
        self.update(|inner| {
            inner.handles.remove(&id);
            inner.list.remove(id)
        });
    }

    /// The connection is over: forget everything and say why, once.
    pub(crate) fn end(&mut self, reason: EndReason) {
        self.placements.clear();
        let first = {
            let mut inner = self.shared.lock();
            let first = !inner.ended;
            inner.ended = true;
            inner.list.clear();
            inner.handles.clear();
            first
        };
        if first {
            self.deliver(Event::Ended(reason));
        }
    }

    /// A window entered (`entered`) or left an output, as part of a batch of
    /// changes its protocol commits later.
    fn output_entered(&mut self, id: WindowId, output: &WlOutput, entered: bool) {
        let Some(global) = Outputs::global_of(output) else {
            return;
        };
        let placement = self
            .placements
            .entry(id)
            .or_insert_with(|| Placement::Outputs(BTreeSet::new()));
        let Placement::Outputs(globals) = placement else {
            return;
        };
        if entered {
            globals.insert(global);
        } else {
            globals.remove(&global);
        }
        let names = self.outputs.names(globals);
        self.update(|inner| {
            inner.list.stage(id, Change::Outputs(names));
            None
        });
    }

    /// A window's geometry changed; its outputs follow at once.
    fn moved(&mut self, id: WindowId, geometry: Rect) {
        self.placements.insert(id, Placement::Geometry(geometry));
        let names = self.names_at(geometry);
        self.update(|inner| inner.list.apply_live(id, Change::Outputs(names)));
    }

    /// The outputs' names or layout changed: every placed window's outputs
    /// are worked out again.
    fn outputs_changed(&mut self) {
        let changes: Vec<_> = self
            .placements
            .iter()
            .map(|(&id, placement)| {
                let names = match placement {
                    Placement::Outputs(globals) => self.outputs.names(globals),
                    &Placement::Geometry(geometry) => self.names_at(geometry),
                };
                (id, names)
            })
            .collect();
        self.update(|inner| {
            changes
                .into_iter()
                .filter_map(|(id, names)| inner.list.apply_live(id, Change::Outputs(names)))
                .collect::<Vec<_>>()
        });
    }

    /// The names of the outputs under `geometry`.
    fn names_at(&self, geometry: Rect) -> BTreeSet<String> {
        layout::outputs_under(geometry, self.outputs.layout())
    }

    /// Binds the globals both backends use: every output (with its xdg-output
    /// description), and one seat.
    fn global_added(
        &mut self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        qh: &QueueHandle<Self>,
    ) {
        if interface == WlOutput::interface().name {
            self.outputs.add(registry, name, version, qh);
        } else if interface == ZxdgOutputManagerV1::interface().name {
            self.outputs.add_manager(registry, name, version, qh);
        } else if interface == WlSeat::interface().name {
            let mut inner = self.shared.lock();
            if inner.seat.is_none() {
                inner.seat = Some(registry.bind(name, SEAT_VERSION, qh, ()));
            }
        }
    }

    /// A global went away; if an output, windows leave it.
    fn global_removed(&mut self, name: u32) {
        if self.outputs.remove(name) {
            for placement in self.placements.values_mut() {
                if let Placement::Outputs(globals) = placement {
                    globals.remove(&name);
                }
            }
            self.outputs_changed();
        }
    }
}

impl Ended for State {
    fn end(&mut self, cause: EndCause) {
        let reason = match cause {
            EndCause::ConnectionLost(error) => EndReason::ConnectionLost(error),
            // A panic, and any cause the client adds later: the stream ends
            // for something other than the connection failing.
            _ => EndReason::HandlerPanicked,
        };
        Self::end(self, reason);
    }
}

impl OutputsHandler for State {
    fn outputs(&mut self) -> &mut Outputs {
        &mut self.outputs
    }

    fn outputs_changed(&mut self) {
        Self::outputs_changed(self);
    }
}

wayland_client::delegate_dispatch!(State: [WlOutput: OutputGlobal] => Outputs);
wayland_client::delegate_dispatch!(State: [ZxdgOutputV1: OutputGlobal] => Outputs);
wayland_client::delegate_dispatch!(State: [ZxdgOutputManagerV1: ()] => Outputs);

/// A state attached and its initial window list loaded.
pub(crate) struct Connected {
    pub(crate) attachment: Attachment,
    pub(crate) shared: Arc<Shared>,
    pub(crate) capabilities: Capabilities,
}

/// Attaches a state to `client`: picks a backend from the globals, binds it
/// and loads the initial list, delivering it to `subscriber` (on the
/// client's thread, before this returns).
pub(crate) fn connect(client: &Client, subscriber: Subscriber) -> Result<Connected, ConnectError> {
    let (attachment, (shared, capabilities)) = client
        .attach(move |setup| set_up(setup, subscriber))
        .map_err(|error| match error {
            AttachError::Setup(error) => error,
            AttachError::Globals(error) => ConnectError::Globals(error),
            AttachError::OnClientThread => ConnectError::OnClientThread,
            // `Ended`, and whatever the client adds later: nothing to attach to.
            _ => ConnectError::ConnectionEnded,
        })?;
    Ok(Connected {
        attachment,
        shared,
        capabilities,
    })
}

/// The setup, on the client's thread: the state, and what the handle keeps.
fn set_up(
    setup: &mut Setup<'_, State>,
    subscriber: Subscriber,
) -> Result<(State, (Arc<Shared>, Capabilities)), ConnectError> {
    let registry = setup.globals().registry().clone();
    let list = setup.globals().contents().clone_list();
    let qh = setup.handle();
    let shared = Arc::new(Shared::default());
    let mut state = State {
        shared: Arc::clone(&shared),
        subscriber,
        detached: setup.detached(),
        outputs: Outputs::default(),
        placements: HashMap::new(),
    };
    // The xdg-output manager first, so that each output is described as it
    // is bound.
    let (managers, others): (Vec<_>, Vec<_>) = list
        .iter()
        .partition(|global| global.interface == ZxdgOutputManagerV1::interface().name);
    for global in managers.into_iter().chain(others) {
        state.global_added(
            &registry,
            global.name,
            &global.interface,
            global.version,
            &qh,
        );
    }
    // Output names first, so that windows entering outputs can be named.
    setup
        .roundtrip(&mut state)
        .map_err(ConnectError::InitialSync)?;

    let output_names = names_available(&list);
    let output_layout = layout_available(&list);
    let (mut capabilities, roundtrips) = bind_backend(setup.globals(), &qh)?;
    capabilities.outputs = output_names
        && match capabilities.protocol {
            Protocol::WlrForeignToplevel => true,
            // KWin reports geometry; outputs are where it lies in the layout.
            Protocol::PlasmaWindowManagement => output_layout,
        };
    for _ in 0..roundtrips {
        setup
            .roundtrip(&mut state)
            .map_err(ConnectError::InitialSync)?;
    }
    Ok((state, (shared, capabilities)))
}

/// Binds the preferred backend `globals` offers. Returns its capabilities and
/// how many exchanges load its initial list.
fn bind_backend(
    globals: &GlobalList,
    qh: &QueueHandle<State>,
) -> Result<(Capabilities, usize), ConnectError> {
    let offered = |interface: &str| {
        globals
            .contents()
            .with_list(|list| list.iter().any(|global| global.interface == interface))
    };
    if offered(wlr::GLOBAL) {
        let (_manager, capabilities) = wlr::bind(globals, qh)?;
        Ok((capabilities, wlr::INITIAL_ROUNDTRIPS))
    } else if offered(plasma::GLOBAL) {
        let (_manager, capabilities) = plasma::bind(globals, qh)?;
        Ok((capabilities, plasma::INITIAL_ROUNDTRIPS))
    } else if globals.contents().with_list(looks_like_kwin) {
        Err(ConnectError::NotGranted)
    } else {
        Err(ConnectError::Unsupported)
    }
}

/// Whether the globals are KWin's: it offers `org_kde_kwin_*` globals to
/// every client, while withholding window management from most.
fn looks_like_kwin(globals: &[wayland_client::globals::Global]) -> bool {
    globals
        .iter()
        .any(|global| global.interface.starts_with("org_kde_kwin_"))
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => state.global_added(registry, name, &interface, version, qh),
            wl_registry::Event::GlobalRemove { name } => state.global_removed(name),
            _ => {}
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wl_seat::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use wayland_client::globals::Global;

    use super::*;

    fn global(interface: &str) -> Global {
        Global {
            name: 1,
            interface: interface.to_owned(),
            version: 1,
        }
    }

    #[test]
    fn kwin_is_recognised_by_its_own_globals() {
        assert!(looks_like_kwin(&[
            global("wl_compositor"),
            global("org_kde_kwin_server_decoration_manager"),
        ]));
        assert!(!looks_like_kwin(&[
            global("wl_compositor"),
            global("wl_seat")
        ]));
    }
}

//! The protocol side: the state attached to the client's connection, the
//! globals both backends share (outputs, a seat), and the choice of
//! backend.
//!
//! [`State`] is what the client's thread dispatches into. What the handle
//! also needs (the list, the protocol objects that actions are sent on) is
//! in [`Shared`], behind a mutex that is never held while the subscriber
//! runs.

mod plasma;
mod released;
mod setup;
mod wlr;

pub(crate) use setup::{Connected, connect};

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use deskrinch_wayland_client::{
    Attachment, Detached, EndCause, Globals, Lifecycle, OutputGlobal, Outputs, OutputsHandler,
    Remaining,
};
use wayland_client::globals::Global;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_plasma::plasma_window_management::client::org_kde_plasma_window::OrgKdePlasmaWindow;
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1;
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1;

use crate::capabilities::{Action, Capabilities};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::ZxdgOutputV1;

use crate::error::{ActionError, EndReason};
use crate::layout::{self, Rect};
use crate::model::{Change, WindowList};
use crate::window::{Event, WindowId};

/// The globals a backend exists for, in order of preference.
pub(crate) const SUPPORTED_GLOBALS: [&str; 2] = [wlr::GLOBAL, plasma::GLOBAL];

/// Activation only names the seat, which any version does; bound up to the
/// version that can release it.
const SEAT_VERSION: u32 = released::SEAT_RELEASE_SINCE;

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

/// The manager a backend bound; once the handle is dropped, only what the
/// released state still waits for.
#[derive(Debug)]
enum Backend {
    /// None yet: the setup binds it after the outputs are named.
    Unbound,
    Wlr(ZwlrForeignToplevelManagerV1),
    /// KWin's manager, kept by no one: it has neither a destructor nor a
    /// `stop`, so nothing is ever sent on it.
    Plasma,
    Released(Remaining),
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
    backend: Backend,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("shared", &self.shared)
            .field("outputs", &self.outputs)
            .field("placements", &self.placements)
            .field("backend", &self.backend)
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

    /// Starts tracking a window the protocol created, unless the state is
    /// released: then the window's handle is destroyed instead.
    fn track(&mut self, id: WindowId, handle: WindowHandle) {
        if self.is_released() {
            match handle {
                WindowHandle::Wlr(toplevel) => toplevel.destroy(),
                WindowHandle::Plasma(window) => window.destroy(),
            }
            return;
        }
        let mut inner = self.shared.lock();
        inner.list.insert(id);
        inner.handles.insert(id, handle);
    }

    /// Whether window `id` is tracked: not one already untracked, nor any
    /// once the state is released.
    fn tracks(&self, id: WindowId) -> bool {
        self.shared.lock().handles.contains_key(&id)
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
    /// changes its protocol commits later. Entering an output whose global
    /// has gone is ignored: it is not listed again.
    fn output_entered(&mut self, id: WindowId, output: &WlOutput, entered: bool) {
        let Some(global) = Outputs::global_of(output) else {
            return;
        };
        if !self.tracks(id) {
            return;
        }
        // An `output_enter` names one of the client's `wl_output`s, of any
        // state on the connection: one that another state bound before this
        // one heard of the global is dropped here. That loses nothing: when
        // this state binds its own `wl_output` for the output, the compositor
        // sends the same existing handle an `output_enter` naming it. Seen
        // under sway and labwc (`WAYLAND_DEBUG` traces of a second state
        // binding its outputs beside a first, which already had a toplevel).
        // KWin's windows report their geometry, not outputs: this does not
        // arise there.
        if entered && !self.outputs.contains(global) {
            return;
        }
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
        if !self.tracks(id) {
            return;
        }
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
    fn bind_global(
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
                inner.seat = Some(registry.bind(name, version.min(SEAT_VERSION), qh, ()));
            }
        }
    }

    /// A global went away; if an output, windows leave it.
    fn forget_global(&mut self, name: u32) {
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

impl Lifecycle for State {
    fn end(&mut self, cause: EndCause) {
        let reason = match cause {
            EndCause::ConnectionLost(error) => EndReason::ConnectionLost(error),
            EndCause::HandlerPanicked => EndReason::HandlerPanicked,
        };
        Self::end(self, reason);
    }

    fn release(&mut self) {
        self.release_all();
    }

    fn released(&self) -> bool {
        self.released_all()
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

impl Globals for State {
    fn global_added(&mut self, registry: &WlRegistry, global: &Global, qh: &QueueHandle<Self>) {
        self.bind_global(registry, global.name, &global.interface, global.version, qh);
    }

    fn global_removed(&mut self, name: u32) {
        self.forget_global(name);
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

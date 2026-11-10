//! The protocol side: one Wayland connection, the globals both backends
//! share (outputs, a seat), and the choice of backend.
//!
//! [`State`] is what the event queue dispatches into. It lives on the
//! dispatch thread; what the handle also needs (the list, the protocol
//! objects that actions are sent on) is in [`Shared`], behind a mutex that is
//! never held while the subscriber runs.

mod plasma;
mod wlr;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols_plasma::plasma_window_management::client::org_kde_plasma_window::OrgKdePlasmaWindow;
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1;

use crate::capabilities::{Action, Capabilities};
use crate::error::{ActionError, ConnectError, EndReason};
use crate::model::WindowList;
use crate::window::{Event, WindowId};

/// The globals a backend exists for, in order of preference.
pub(crate) const SUPPORTED_GLOBALS: [&str; 2] = [wlr::GLOBAL, plasma::GLOBAL];

/// `wl_output.name` arrived in version 4.
const OUTPUT_VERSION: u32 = 4;
/// `wl_output.release` arrived in version 3.
const OUTPUT_RELEASE_SINCE: u32 = 3;
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

/// A bound `wl_output`.
#[derive(Debug)]
struct Output {
    proxy: WlOutput,
    name: Option<String>,
}

/// The dispatch state of one connection.
pub(crate) struct State {
    shared: Arc<Shared>,
    subscriber: Subscriber,
    /// Bound outputs, by registry name.
    outputs: HashMap<u32, Output>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("shared", &self.shared)
            .field("outputs", &self.outputs)
            .finish_non_exhaustive()
    }
}

impl State {
    /// Runs `change` under the lock, then delivers what it produced.
    fn update<E: IntoIterator<Item = Event>>(&mut self, change: impl FnOnce(&mut Inner) -> E) {
        let events: Vec<_> = change(&mut self.shared.lock()).into_iter().collect();
        for event in events {
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
        self.update(|inner| {
            inner.handles.remove(&id);
            inner.list.remove(id)
        });
    }

    /// The connection is over: forget everything and say why, once.
    pub(crate) fn end(&mut self, reason: EndReason) {
        let first = {
            let mut inner = self.shared.lock();
            let first = !inner.ended;
            inner.ended = true;
            inner.list.clear();
            inner.handles.clear();
            first
        };
        if first {
            (self.subscriber)(Event::Ended(reason));
        }
    }

    /// Whether the connection has ended.
    pub(crate) fn ended(&self) -> bool {
        self.shared.lock().ended
    }

    /// The name of a bound output, once the compositor has sent it.
    fn output_name(&self, output: &WlOutput) -> Option<String> {
        let global = output.data::<u32>()?;
        self.outputs.get(global)?.name.clone()
    }

    /// Binds the globals both backends use: every output, and one seat.
    fn global_added(
        &mut self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        qh: &QueueHandle<Self>,
    ) {
        if interface == WlOutput::interface().name {
            let proxy = registry.bind(name, version.min(OUTPUT_VERSION), qh, name);
            self.outputs.insert(name, Output { proxy, name: None });
        } else if interface == WlSeat::interface().name {
            let mut inner = self.shared.lock();
            if inner.seat.is_none() {
                inner.seat = Some(registry.bind(name, SEAT_VERSION, qh, ()));
            }
        }
    }

    /// An output went away: windows leave it.
    fn global_removed(&mut self, name: u32) {
        let Some(output) = self.outputs.remove(&name) else {
            return;
        };
        if output.proxy.version() >= OUTPUT_RELEASE_SINCE {
            output.proxy.release();
        }
        if let Some(output_name) = output.name {
            self.update(|inner| inner.list.forget_output(&output_name));
        }
    }
}

/// A connection whose initial window list is loaded, ready to be dispatched.
pub(crate) struct Connected {
    pub(crate) connection: Connection,
    pub(crate) queue: EventQueue<State>,
    pub(crate) state: State,
    pub(crate) shared: Arc<Shared>,
    pub(crate) capabilities: Capabilities,
}

/// Picks a backend from `connection`'s globals, binds it and loads the
/// initial list, delivering it to `subscriber` on this thread.
pub(crate) fn connect(
    connection: Connection,
    subscriber: Subscriber,
) -> Result<Connected, ConnectError> {
    let (globals, mut queue) = registry_queue_init::<State>(&connection)?;
    let qh = queue.handle();
    let shared = Arc::new(Shared::default());
    let mut state = State {
        shared: Arc::clone(&shared),
        subscriber,
        outputs: HashMap::new(),
    };
    for global in globals.contents().clone_list() {
        state.global_added(
            globals.registry(),
            global.name,
            &global.interface,
            global.version,
            &qh,
        );
    }
    // Output names first, so that windows entering outputs can be named.
    queue
        .roundtrip(&mut state)
        .map_err(ConnectError::InitialSync)?;

    let (capabilities, roundtrips) = bind_backend(&globals, &qh)?;
    for _ in 0..roundtrips {
        queue
            .roundtrip(&mut state)
            .map_err(ConnectError::InitialSync)?;
    }
    Ok(Connected {
        connection,
        queue,
        state,
        shared,
        capabilities,
    })
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

impl Dispatch<WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event
            && let Some(output) = state.outputs.get_mut(global)
        {
            output.name = Some(name);
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

/// The `wl_display.sync` the handle sends to wake the dispatch thread.
impl Dispatch<WlCallback, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlCallback,
        _: wl_callback::Event,
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

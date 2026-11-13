//! The protocol side: the state attached to the client's connection, the
//! outputs, and the choice of backend.
//!
//! [`State`] is what the client's thread dispatches into; what the handle
//! also needs (the list, the protocol objects that
//! actions are sent on) is in [`Shared`], behind a mutex that is never held
//! while the subscriber runs.

mod ext;
mod plasma;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use deskrinch_wayland_client::{
    AttachError, Attachment, Client, Detached, EndCause, Ended, OutputGlobal, Outputs,
    OutputsHandler, Setup, names_available,
};
use wayland_client::globals::{GlobalList, GlobalListContents};
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1::ExtWorkspaceHandleV1;
use wayland_protocols::ext::workspace::v1::client::ext_workspace_manager_v1::ExtWorkspaceManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::ZxdgOutputV1;
use wayland_protocols_plasma::plasma_virtual_desktop::client::org_kde_plasma_virtual_desktop::OrgKdePlasmaVirtualDesktop;

use crate::capabilities::{Action, Capabilities};
use crate::error::{ActionError, ConnectError, EndReason};
use crate::model::WorkspaceList;
use crate::workspace::{Event, WorkspaceId};

/// The globals a backend exists for, in order of preference: the standard
/// protocol first.
pub(crate) const SUPPORTED_GLOBALS: [&str; 2] = [ext::GLOBAL, plasma::GLOBAL];

/// What receives [`Event`]s.
pub(crate) type Subscriber = Box<dyn Fn(Event) + Send>;

/// The protocol object a workspace's actions are sent on.
#[derive(Debug)]
enum Handle {
    Ext(ExtWorkspaceHandleV1),
    Plasma(OrgKdePlasmaVirtualDesktop),
}

/// What the client's thread and the handle share.
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
    pub(crate) list: WorkspaceList,
    handles: HashMap<WorkspaceId, Handle>,
    /// `ext_workspace_v1`'s manager, which commits requests.
    ext_manager: Option<ExtWorkspaceManagerV1>,
    ended: bool,
}

impl Inner {
    /// Sends `action` for `id`; the caller flushes.
    pub(crate) fn request(
        &self,
        capabilities: Capabilities,
        id: WorkspaceId,
        action: Action,
    ) -> Result<(), ActionError> {
        if self.ended {
            return Err(ActionError::Ended);
        }
        if !capabilities.supports(action) {
            return Err(ActionError::Unsupported(action));
        }
        let workspace = self.list.get(id).ok_or(ActionError::UnknownWorkspace(id))?;
        if !workspace.actions.contains(action.flag()) {
            return Err(ActionError::NotAllowed {
                action,
                workspace: id,
            });
        }
        match self.handles.get(&id) {
            Some(Handle::Ext(handle)) => {
                ext::perform(handle, self.ext_manager.as_ref(), action);
                Ok(())
            }
            Some(Handle::Plasma(desktop)) => {
                plasma::perform(desktop, action);
                Ok(())
            }
            None => Err(ActionError::UnknownWorkspace(id)),
        }
    }
}

/// What a backend keeps on the client's thread.
#[derive(Debug)]
enum Backend {
    Ext(ext::Tracking),
    Plasma(plasma::Tracking),
}

/// The state attached to the client's connection.
pub(crate) struct State {
    shared: Arc<Shared>,
    subscriber: Subscriber,
    /// Set once the handle is dropped: the subscriber hears nothing more.
    detached: Detached,
    outputs: Outputs,
    backend: Backend,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("shared", &self.shared)
            .field("outputs", &self.outputs)
            .field("backend", &self.backend)
            .finish_non_exhaustive()
    }
}

impl State {
    /// Delivers what the list has staged since its last commit.
    fn commit(&mut self) {
        let events = self.shared.lock().list.commit();
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

    /// The connection is over: forget everything and say why, once.
    fn end(&mut self, reason: EndReason) {
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

    /// Binds the outputs the backends name groups' outputs from.
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
        }
    }

    /// A global went away; if an output, the groups' outputs follow.
    fn global_removed(&mut self, name: u32) {
        if self.outputs.remove(name) {
            OutputsHandler::outputs_changed(self);
        }
    }
}

impl Ended for State {
    fn end(&mut self, cause: EndCause) {
        let reason = match cause {
            EndCause::ConnectionLost(error) => EndReason::ConnectionLost(error),
            EndCause::HandlerPanicked => EndReason::HandlerPanicked,
        };
        Self::end(self, reason);
    }
}

impl OutputsHandler for State {
    fn outputs(&mut self) -> &mut Outputs {
        &mut self.outputs
    }

    /// An output was named, renamed or went away: the groups' outputs are
    /// delivered alone. The protocol's open batch, if any, waits for its
    /// own end: a group's outputs and workspaces are never delivered half
    /// way through a batch.
    fn outputs_changed(&mut self) {
        let events = {
            let mut inner = self.shared.lock();
            let list = &mut inner.list;
            match &self.backend {
                Backend::Ext(tracking) => {
                    tracking.stage_outputs(list, |globals| self.outputs.names(globals));
                }
                Backend::Plasma(tracking) => tracking.stage_outputs(list, &self.outputs),
            }
            list.commit_groups()
        };
        for event in events {
            self.deliver(event);
        }
    }
}

wayland_client::delegate_dispatch!(State: [WlOutput: OutputGlobal] => Outputs);
wayland_client::delegate_dispatch!(State: [ZxdgOutputV1: OutputGlobal] => Outputs);
wayland_client::delegate_dispatch!(State: [ZxdgOutputManagerV1: ()] => Outputs);

/// A state attached and its initial workspaces loaded.
pub(crate) struct Connected {
    pub(crate) attachment: Attachment,
    pub(crate) shared: Arc<Shared>,
    pub(crate) capabilities: Capabilities,
}

/// Attaches a state to `client`: picks a backend from the globals, binds it
/// and loads the initial workspaces, delivering them to `subscriber` (on the
/// client's thread, before this returns).
pub(crate) fn connect(client: &Client, subscriber: Subscriber) -> Result<Connected, ConnectError> {
    let (attachment, (shared, capabilities)) = client
        .attach(move |setup| set_up(setup, subscriber))
        .map_err(|error| match error {
            AttachError::Setup(error) => error,
            AttachError::Globals(error) => ConnectError::Globals(error),
            AttachError::OnClientThread => ConnectError::OnClientThread,
            AttachError::Ended => ConnectError::ConnectionEnded,
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
    let qh = setup.handle();
    let registry = setup.globals().registry().clone();
    let list = setup.globals().contents().clone_list();
    let shared = Arc::new(Shared::default());
    let (backend, mut capabilities, roundtrips) = bind_backend(setup.globals(), &shared, &qh)?;
    capabilities.outputs = names_available(&list);
    let mut state = State {
        shared: Arc::clone(&shared),
        subscriber,
        detached: setup.detached(),
        outputs: Outputs::default(),
        backend,
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
    for _ in 0..roundtrips {
        setup
            .roundtrip(&mut state)
            .map_err(ConnectError::InitialSync)?;
    }
    Ok((state, (shared, capabilities)))
}

/// Binds the preferred backend `globals` offers. Returns its tracking, its
/// capabilities and how many exchanges load its initial workspaces.
fn bind_backend(
    globals: &GlobalList,
    shared: &Shared,
    qh: &QueueHandle<State>,
) -> Result<(Backend, Capabilities, usize), ConnectError> {
    let offered = |interface: &str| {
        globals
            .contents()
            .with_list(|list| list.iter().any(|global| global.interface == interface))
    };
    if offered(ext::GLOBAL) {
        let manager = ext::bind(globals, qh)?;
        shared.lock().ext_manager = Some(manager);
        Ok((
            Backend::Ext(ext::Tracking::default()),
            ext::CAPABILITIES,
            ext::INITIAL_ROUNDTRIPS,
        ))
    } else if offered(plasma::GLOBAL) {
        let tracking = plasma::bind(globals, shared, qh)?;
        Ok((
            Backend::Plasma(tracking),
            plasma::CAPABILITIES,
            plasma::INITIAL_ROUNDTRIPS,
        ))
    } else {
        Err(ConnectError::Unsupported)
    }
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

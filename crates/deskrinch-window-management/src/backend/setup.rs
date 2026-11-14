//! Attaching the state: the globals both backends use bound, the outputs
//! named, then the preferred backend bound and its first list loaded.

use std::collections::HashMap;
use std::sync::Arc;

use deskrinch_wayland_client::{
    AttachError, Client, Outputs, Setup, layout_available, names_available,
};
use wayland_client::globals::{Global, GlobalList};
use wayland_client::{Proxy, QueueHandle};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;

use super::{Attachment, Backend, Shared, State, Subscriber, plasma, wlr};
use crate::capabilities::{Capabilities, Protocol};
use crate::error::ConnectError;

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
        backend: Backend::Unbound,
    };
    // The xdg-output manager first, so that each output is described as it
    // is bound.
    let (managers, others): (Vec<_>, Vec<_>) = list
        .iter()
        .partition(|global| global.interface == ZxdgOutputManagerV1::interface().name);
    for global in managers.into_iter().chain(others) {
        state.bind_global(
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
    let (backend, mut capabilities, roundtrips) = bind_backend(setup.globals(), &qh)?;
    state.backend = backend;
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

/// Binds the preferred backend `globals` offers. Returns its manager, its
/// capabilities and how many exchanges load its initial list.
fn bind_backend(
    globals: &GlobalList,
    qh: &QueueHandle<State>,
) -> Result<(Backend, Capabilities, usize), ConnectError> {
    let offered = |interface: &str| {
        globals
            .contents()
            .with_list(|list| list.iter().any(|global| global.interface == interface))
    };
    if offered(wlr::GLOBAL) {
        let (manager, capabilities) = wlr::bind(globals, qh)?;
        Ok((Backend::Wlr(manager), capabilities, wlr::INITIAL_ROUNDTRIPS))
    } else if offered(plasma::GLOBAL) {
        let (_manager, capabilities) = plasma::bind(globals, qh)?;
        Ok((Backend::Plasma, capabilities, plasma::INITIAL_ROUNDTRIPS))
    } else if globals.contents().with_list(looks_like_kwin) {
        Err(ConnectError::NotGranted)
    } else {
        Err(ConnectError::Unsupported)
    }
}

/// Whether the globals are KWin's: it offers `org_kde_kwin_*` globals to
/// every client, while withholding window management from most.
fn looks_like_kwin(globals: &[Global]) -> bool {
    globals
        .iter()
        .any(|global| global.interface.starts_with("org_kde_kwin_"))
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

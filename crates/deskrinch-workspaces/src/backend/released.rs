//! A released state: what it lets go of when its handle is dropped, and
//! what can still reach it.
//!
//! Released mode is one place, [`Backend::Released`]: a released state has
//! no backend tracking, so every handler finds none and returns, and the
//! manager's handlers destroy a workspace or group it announces late
//! instead of listing it.

use deskrinch_wayland_client::Remaining;

use super::{Backend, Handle, State};

impl State {
    /// Lets go of everything: the list and the outputs, each workspace and
    /// group handle destroyed, and the ext manager told to `stop`; KWin's
    /// objects, which have no destructor, are only forgotten.
    pub(super) fn release_all(&mut self) {
        let mut remaining = Remaining::default();
        if !self.outputs.release() {
            remaining.linger();
        }
        let (ext_manager, handles) = {
            let mut inner = self.shared.lock();
            inner.ended = true;
            inner.list.clear();
            (inner.ext_manager.take(), std::mem::take(&mut inner.handles))
        };
        for handle in handles.into_values() {
            match handle {
                Handle::Ext(workspace) => workspace.destroy(),
                // `org_kde_plasma_virtual_desktop` has no destructor.
                Handle::Plasma(_) => {}
            }
        }
        match std::mem::replace(&mut self.backend, Backend::Released(remaining)) {
            Backend::Ext(tracking) => {
                tracking.release();
                // After the handles: the manager takes no request after
                // `stop`.
                if let Some(manager) = ext_manager {
                    manager.stop();
                    remaining.await_finished();
                }
            }
            // The manager and its desktops have no destructor.
            Backend::Plasma(_) => remaining.linger(),
            Backend::Released(_) => return,
        }
        self.backend = Backend::Released(remaining);
    }

    /// Whether nothing the state bound can reach it any more.
    pub(super) fn released_all(&self) -> bool {
        matches!(&self.backend, Backend::Released(remaining) if remaining.is_empty())
    }
}

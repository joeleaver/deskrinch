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
        // Once only: a second release must not take a lingering state for a
        // released one (everything is already let go of, so it would look
        // like nothing remains).
        if matches!(self.backend, Backend::Released(_)) {
            return;
        }
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
            // Not reached past the guard above; kept as it was if it were.
            Backend::Released(earlier) => {
                self.backend = Backend::Released(earlier);
                return;
            }
        }
        self.backend = Backend::Released(remaining);
    }

    /// Whether nothing the state bound can reach it any more.
    pub(super) fn released_all(&self) -> bool {
        matches!(&self.backend, Backend::Released(remaining) if remaining.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use deskrinch_wayland_client::{Detached, Outputs};

    use super::*;

    /// A state released already, still holding something without a
    /// destructor.
    fn lingering() -> State {
        let mut remaining = Remaining::default();
        remaining.linger();
        let backend = Backend::Released(remaining);
        State {
            shared: Arc::default(),
            subscriber: Box::new(|_| {}),
            detached: Detached::default(),
            outputs: Outputs::default(),
            backend,
        }
    }

    #[test]
    fn a_second_release_leaves_a_lingering_state_unreleased() {
        let mut state = lingering();
        assert!(!state.released_all());
        state.release_all();
        assert!(
            !state.released_all(),
            "still lingering after a second release"
        );
    }
}

//! A released state: what it lets go of when its handle is dropped, and
//! what can still reach it.
//!
//! Released mode is one place, [`Backend::Released`]: a released state
//! tracks no window ([`State::track`] refuses, and the manager's handler
//! destroys a toplevel announced late), and the windows' own handlers find
//! nothing to update.

use deskrinch_wayland_client::Remaining;
use wayland_client::Proxy;

use super::{Backend, State, WindowHandle};

/// `wl_seat.release` arrived in version 5.
pub(super) const SEAT_RELEASE_SINCE: u32 = 5;

impl State {
    /// Lets go of everything: the list, the outputs and the seat, each
    /// window's handle destroyed, and the wlr manager told to `stop`;
    /// KWin's manager, which has no destructor, is only forgotten.
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
        self.placements.clear();
        let (handles, seat) = {
            let mut inner = self.shared.lock();
            inner.ended = true;
            inner.list.clear();
            (std::mem::take(&mut inner.handles), inner.seat.take())
        };
        for handle in handles.into_values() {
            match handle {
                WindowHandle::Wlr(toplevel) => toplevel.destroy(),
                // Bound at version 13 or newer: `destroy` came in 4.
                WindowHandle::Plasma(window) => window.destroy(),
            }
        }
        if let Some(seat) = seat {
            if seat.version() >= SEAT_RELEASE_SINCE {
                seat.release();
            } else {
                remaining.linger();
            }
        }
        match std::mem::replace(&mut self.backend, Backend::Released(remaining)) {
            Backend::Wlr(manager) => {
                // After the handles: the manager takes no request after
                // `stop`.
                manager.stop();
                remaining.await_finished();
            }
            // `org_kde_plasma_window_management` has no destructor.
            Backend::Plasma => remaining.linger(),
            Backend::Unbound => {}
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

    /// Whether the handle has been dropped and the state released.
    pub(super) fn is_released(&self) -> bool {
        matches!(self.backend, Backend::Released(_))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
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
            placements: HashMap::new(),
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

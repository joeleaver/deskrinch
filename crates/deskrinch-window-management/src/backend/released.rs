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
            Backend::Released(_) => return,
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

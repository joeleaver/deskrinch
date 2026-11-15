//! How an attached state learns that its connection has ended, and how it
//! lets go of what it bound when its handle is dropped.

use std::sync::Arc;

use wayland_client::DispatchError;

/// Why the client's thread ended the attached states.
#[derive(Debug, Clone)]
pub enum EndCause {
    /// The connection failed: the compositor went away, or sent something
    /// that ended it (a protocol error raised by any user of the
    /// connection).
    ConnectionLost(Arc<DispatchError>),
    /// A handler of some state on the connection panicked, an attached
    /// state's or a releasing one's. The panic was reported where it
    /// happened (the panic hook); every attached state is ended, since the
    /// connection can no longer be relied on to be dispatched.
    HandlerPanicked,
}

/// What every state attached to a [`Client`](crate::Client) does at the
/// ends of its life, called on the client's thread. Every method is
/// required: a state that could forget to release what it bound would
/// leave the compositor sending it events for as long as the connection
/// lasts.
pub trait Lifecycle {
    /// The connection has ended for this state, for `cause`. Called exactly
    /// once for an attached state, which is then dropped; never for one
    /// whose handle was dropped before.
    fn end(&mut self, cause: EndCause);

    /// The state's [`Attachment`](crate::Attachment) was dropped, on a
    /// connection still alive: tell the compositor to stop, with the
    /// requests each protocol has for it (a manager's `stop`, a handle's
    /// `destroy`, [`Outputs::release`](crate::Outputs::release)), and keep
    /// nothing more. Called once.
    ///
    /// The client's thread then keeps dispatching the state, as a releasing
    /// state, until [`released`](Self::released) says nothing can reach it
    /// any more. Meanwhile its handlers create nothing and keep nothing:
    /// whatever the compositor sends is consumed and dropped, and an object
    /// it announces late (a toplevel or a workspace made before it read the
    /// `stop`) is destroyed as it arrives.
    fn release(&mut self);

    /// Whether nothing the state bound can still send it events: every
    /// object destroyed, and every manager's `finished` received. The
    /// client's thread drops the state once it says so, and never before.
    ///
    /// A state that holds an object without a destructor (KWin's
    /// window-management and virtual-desktop managers, a `wl_seat` or
    /// `wl_output` too old to be released) never says so: it is dispatched,
    /// and its events thrown away, for as long as the connection lasts.
    fn released(&self) -> bool;
}

/// What a released state still waits for, for its
/// [`Lifecycle::released`]: a manager's `finished`, or nothing ever,
/// because something it bound has no destructor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Remaining {
    awaiting_finished: bool,
    lingering: bool,
}

impl Remaining {
    /// A manager was told to `stop`: its `finished` is awaited.
    pub fn await_finished(&mut self) {
        self.awaiting_finished = true;
    }

    /// The manager's `finished` arrived.
    pub fn finished(&mut self) {
        self.awaiting_finished = false;
    }

    /// Something stays bound that has no destructor: nothing ever releases
    /// the state.
    pub fn linger(&mut self) {
        self.lingering = true;
    }

    /// Whether nothing is left that can reach the state.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.awaiting_finished && !self.lingering
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_once_finished_unless_something_lingers() {
        let mut remaining = Remaining::default();
        assert!(remaining.is_empty());
        remaining.await_finished();
        assert!(!remaining.is_empty());
        remaining.finished();
        assert!(remaining.is_empty());
        remaining.linger();
        assert!(!remaining.is_empty(), "nothing releases what lingers");
    }
}

//! How an attached state learns that its connection has ended.

use std::sync::Arc;

use wayland_client::DispatchError;

/// Why the client's thread ended the attached states.
#[derive(Debug, Clone)]
pub enum EndCause {
    /// The connection failed: the compositor went away, or sent something
    /// that ended it (a protocol error raised by any user of the
    /// connection).
    ConnectionLost(Arc<DispatchError>),
    /// A handler of some state attached to the connection panicked. The
    /// panic was reported where it happened (the panic hook); every state
    /// on the connection is ended, since the connection can no longer be
    /// relied on to be dispatched.
    HandlerPanicked,
}

/// A state attached to a [`Client`](crate::Client) hears through this that
/// its stream has ended. The client's thread calls it exactly once per
/// attached state, then drops the state.
pub trait Ended {
    /// The connection has ended for this state, for `cause`.
    fn end(&mut self, cause: EndCause);
}

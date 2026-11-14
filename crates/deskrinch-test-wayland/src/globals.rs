//! The compositor's globals, listed with a deadline.
//!
//! `wayland_client::globals::registry_queue_init` waits for the compositor
//! with a blocking round trip. [`Globals::list`] does the same within a
//! `deskrinch_test_wait::Deadline`, and keeps the list itself (on the registry's own object
//! data, so the client's state needs no registry handler).

use std::ops::RangeInclusive;
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex, PoisonError};

use wayland_client::backend::protocol::Message;
use wayland_client::backend::{Backend, ObjectData, ObjectId};
use wayland_client::protocol::wl_display;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};

use deskrinch_test_wait::Deadline;

use crate::wait::{WaitError, roundtrip};

/// One global the compositor announced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Global {
    /// Its name, to bind it by.
    pub name: u32,
    /// Its interface.
    pub interface: String,
    /// The newest version the compositor offers.
    pub version: u32,
}

/// Why a global could not be bound.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BindError {
    /// The compositor does not offer it.
    #[error("the compositor offers no {0}")]
    Missing(&'static str),
    /// The compositor offers it at a version older than needed.
    #[error("the compositor offers {interface} at version {offered}, older than {needed}")]
    TooOld {
        /// The interface.
        interface: &'static str,
        /// The version offered.
        offered: u32,
        /// The oldest version that will do.
        needed: u32,
    },
}

/// The registry, and the globals it has announced so far.
#[derive(Debug)]
pub struct Globals {
    registry: WlRegistry,
    list: Arc<Listed>,
}

impl Globals {
    /// Asks for the registry and waits, until `deadline`, for the
    /// compositor to list its globals.
    ///
    /// # Errors
    ///
    /// [`WaitError::TimedOut`] when the compositor does not answer in time;
    /// otherwise when the connection fails.
    pub fn list<S>(
        connection: &Connection,
        queue: &mut EventQueue<S>,
        state: &mut S,
        deadline: Deadline,
    ) -> Result<Self, WaitError> {
        let list = Arc::new(Listed::default());
        let data: Arc<dyn ObjectData> = Arc::<Listed>::clone(&list);
        let registry = connection
            .display()
            .send_constructor::<WlRegistry>(wl_display::Request::GetRegistry {}, data)
            .map_err(|_| WaitError::Closed)?;
        roundtrip(connection, queue, state, deadline)?;
        Ok(Self { registry, list })
    }

    /// Every global announced so far, in the order announced.
    #[must_use]
    pub fn all(&self) -> Vec<Global> {
        self.list
            .globals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether a global of `interface` was announced.
    #[must_use]
    pub fn offers(&self, interface: &str) -> bool {
        self.all()
            .iter()
            .any(|global| global.interface == interface)
    }

    /// Binds the first global of `I`'s interface, at the newest version in
    /// `versions` the compositor offers.
    ///
    /// # Errors
    ///
    /// When there is none, or only one older than `versions` allows.
    ///
    /// # Panics
    ///
    /// When `versions` goes beyond what `I`'s bindings know: a mistake in
    /// the caller, not something the compositor can cause.
    pub fn bind<I, S, U>(
        &self,
        qh: &QueueHandle<S>,
        versions: RangeInclusive<u32>,
        data: U,
    ) -> Result<I, BindError>
    where
        I: Proxy + 'static,
        S: Dispatch<I, U> + 'static,
        U: Send + Sync + 'static,
    {
        let interface = I::interface();
        assert!(
            *versions.end() <= interface.version,
            "{} is known only up to version {}",
            interface.name,
            interface.version
        );
        let global = self
            .all()
            .into_iter()
            .find(|global| global.interface == interface.name)
            .ok_or(BindError::Missing(interface.name))?;
        if global.version < *versions.start() {
            return Err(BindError::TooOld {
                interface: interface.name,
                offered: global.version,
                needed: *versions.start(),
            });
        }
        Ok(self.bind_global(&global, global.version.min(*versions.end()), qh, data))
    }

    /// Binds `global` at `version`.
    pub fn bind_global<I, S, U>(
        &self,
        global: &Global,
        version: u32,
        qh: &QueueHandle<S>,
        data: U,
    ) -> I
    where
        I: Proxy + 'static,
        S: Dispatch<I, U> + 'static,
        U: Send + Sync + 'static,
    {
        self.registry.bind(global.name, version, qh, data)
    }
}

/// The registry's object data: the list of globals, kept up to date as the
/// compositor announces and removes them.
#[derive(Debug, Default)]
struct Listed {
    globals: Mutex<Vec<Global>>,
}

impl ObjectData for Listed {
    fn event(
        self: Arc<Self>,
        backend: &Backend,
        message: Message<ObjectId, OwnedFd>,
    ) -> Option<Arc<dyn ObjectData>> {
        let connection = Connection::from_backend(backend.clone());
        let mut globals = self.globals.lock().unwrap_or_else(PoisonError::into_inner);
        match WlRegistry::parse_event(&connection, message) {
            Ok((
                _,
                wl_registry::Event::Global {
                    name,
                    interface,
                    version,
                },
            )) => globals.push(Global {
                name,
                interface,
                version,
            }),
            Ok((_, wl_registry::Event::GlobalRemove { name })) => {
                globals.retain(|global| global.name != name);
            }
            // Nothing else is sent to a registry; a malformed event is
            // already a protocol error on the connection.
            _ => {}
        }
        None
    }

    fn destroyed(&self, _: ObjectId) {}
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    use super::*;

    #[derive(Debug)]
    struct Nothing;

    #[test]
    fn a_compositor_that_lists_nothing_times_out() {
        let (ours, _theirs) = UnixStream::pair().unwrap();
        let connection = Connection::from_socket(ours).unwrap();
        let mut queue = connection.new_event_queue::<Nothing>();
        let error = Globals::list(
            &connection,
            &mut queue,
            &mut Nothing,
            Deadline::after(Duration::from_millis(200)),
        )
        .unwrap_err();
        assert!(matches!(error, WaitError::TimedOut(_)), "{error:?}");
    }
}

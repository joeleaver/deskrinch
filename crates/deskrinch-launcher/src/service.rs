//! The launcher's interface on the session bus, which summons it, and the
//! name that makes it one per session.
//!
//! `org.deskrinch.Launcher` owns `/org/deskrinch/Launcher`, with the
//! interface `org.deskrinch.Launcher1`:
//!
//! - `Toggle() -> b`: shows the launcher if hidden, hides it if shown, and
//!   answers whether it is now shown;
//! - `Show()`, `Hide()`: shows or hides it; one already shown or hidden
//!   stays so;
//! - `ForgetStarts()`: deletes the record of the applications started,
//!   and returns once it is deleted;
//! - the property `Shown` (`b`, read-only): whether it is shown, or about
//!   to be, read on demand (no `PropertiesChanged` is sent for it).
//!
//! The name is requested without queueing or replacement: a second
//! launcher finds it taken ([`ServeError::Running`]) and leaves the first
//! one to answer.

use std::sync::Arc;

use deskrinch_bus::Bus;
use zbus::Connection;
use zbus::fdo::{RequestNameFlags, RequestNameReply};

use crate::visibility::Visibility;

/// The launcher's well-known name.
pub const NAME: &str = "org.deskrinch.Launcher";
/// Its object.
pub const PATH: &str = "/org/deskrinch/Launcher";
/// Its interface.
pub const INTERFACE: &str = "org.deskrinch.Launcher1";

/// The served object.
struct Launcher {
    visibility: Arc<Visibility>,
    forget_starts: Box<dyn Fn() + Send + Sync>,
}

#[zbus::interface(name = "org.deskrinch.Launcher1")]
impl Launcher {
    fn toggle(&self) -> bool {
        self.visibility.toggle()
    }

    fn show(&self) {
        self.visibility.show();
    }

    fn hide(&self) {
        self.visibility.hide();
    }

    /// Blocks the connection's thread while the record is deleted: a file
    /// removal, which the caller waits on anyway.
    fn forget_starts(&self) {
        (self.forget_starts)();
    }

    /// Whether it is shown, or about to be. Read on demand: no signal says
    /// when it changes.
    #[zbus(property(emits_changed_signal = "false"))]
    fn shown(&self) -> bool {
        self.visibility.is_shown()
    }
}

/// Why the launcher could not take its place on the bus.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// Another launcher owns the name.
    #[error("a launcher is already running on this bus")]
    Running,
    /// The bus could not be reached, or would not serve the object or give
    /// the name.
    #[error("cannot serve the launcher on the bus")]
    Bus(#[source] zbus::Error),
}

/// The launcher on the bus while this lives: its object served and its
/// name owned. Dropping it releases the name.
#[derive(Debug)]
pub struct Service {
    connection: Connection,
}

impl Service {
    /// Serves the interface on `bus` and takes the name; calls change
    /// `visibility`, and `ForgetStarts` calls `forget_starts`, which
    /// returns once the record is deleted. The connection runs on zbus's
    /// own thread.
    ///
    /// # Errors
    ///
    /// [`ServeError::Running`] when another launcher has the name;
    /// [`ServeError::Bus`] when the bus fails.
    pub fn start(
        bus: &Bus,
        visibility: Arc<Visibility>,
        forget_starts: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, ServeError> {
        let object = Launcher {
            visibility,
            forget_starts: Box::new(forget_starts),
        };
        let connection = zbus::block_on(async {
            let connection = bus
                .builder()
                .serve_at(PATH, object)
                .build()
                .await
                .map_err(ServeError::Bus)?;
            let reply = connection
                .request_name_with_flags(NAME, RequestNameFlags::DO_NOT_QUEUE)
                .await
                .map_err(|error| match error {
                    zbus::Error::NameTaken => ServeError::Running,
                    other => ServeError::Bus(other),
                })?;
            match reply {
                RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(connection),
                RequestNameReply::Exists | RequestNameReply::InQueue => Err(ServeError::Running),
            }
        })?;
        Ok(Self { connection })
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        // Closing releases the name; it fails only when the connection is
        // already gone, which released it too.
        let _closed = zbus::block_on(self.connection.clone().close());
    }
}

//! [`Kwin`]: the handle every question to KWin is asked through, and the
//! proof that its bus is one of ours.
//!
//! What must never happen is a question on the owner's bus. There are two
//! ways to know a bus is ours, and they are the handle's only makers:
//!
//! - [`Kwin::here`]: this process runs inside an isolated session of ours
//!   whose compositor is KWin (the session's own check), and the handle is
//!   for that session's bus;
//! - [`Kwin::on`]: the address is a private bus of ours
//!   ([`deskrinch_test_session::bus::is_private`], exact), for a process
//!   that holds the address of a session it started itself: xtask asking
//!   the session it is starting, a test asking the desktop it started.
//!
//! Both refuse with a typed error before any connection. They are the bus
//! proof's own two makers ([`deskrinch_test_dbus::OurBus`]), with KWin's
//! compositor required of the first; the handle holds the proof, so a
//! question cannot be asked without one, and every connection it makes is
//! to the address it was proved for.

use std::time::Duration;

use deskrinch_test_dbus::{NotOurs, OurBus};
use deskrinch_test_session::{Environment, KWIN};
use zbus::Connection;
use zbus::connection::Builder;

use crate::Error;

/// KWin on a bus of ours: the only way to ask it anything.
#[derive(Debug, Clone)]
pub struct Kwin {
    /// The bus, proved ours.
    bus: OurBus,
}

impl Kwin {
    /// KWin in the isolated session this process runs in.
    ///
    /// # Errors
    ///
    /// [`Error::NotIsolated`] outside an isolated session of ours;
    /// [`Error::NotKwin`] in one whose compositor is not KWin. Nothing is
    /// connected to.
    pub fn here() -> Result<Self, Error> {
        Self::in_environment(&Environment::read())
    }

    /// [`here`](Self::here), for `environment`.
    fn in_environment(environment: &Environment) -> Result<Self, Error> {
        let (bus, isolated) = OurBus::in_session(environment).map_err(refused)?;
        if isolated.compositor == KWIN {
            Ok(Self { bus })
        } else {
            Err(Error::NotKwin {
                compositor: isolated.compositor,
            })
        }
    }

    /// KWin on the bus at `address`, which must be a private bus of ours.
    ///
    /// # Errors
    ///
    /// [`Error::NotOurBus`] for any other address. Nothing is connected to.
    pub fn on(address: &str) -> Result<Self, Error> {
        OurBus::at(address).map(|bus| Self { bus }).map_err(refused)
    }

    /// The same, each call it makes limited to `patience`.
    #[must_use]
    pub fn patience(self, patience: Duration) -> Self {
        Self {
            bus: self.bus.patience(patience),
        }
    }

    /// A connection to its bus, to finish building.
    pub(crate) fn builder(&self) -> Builder<'_> {
        self.bus.builder()
    }

    /// A connection to its bus.
    pub(crate) async fn connect(&self) -> Result<Connection, Error> {
        Ok(self.bus.connect().await?)
    }
}

/// The handle's refusal, for the proof's.
fn refused(why: NotOurs) -> Error {
    match why {
        NotOurs::NotIsolated(why) => Error::NotIsolated(why),
        NotOurs::NotPrivate { address } => Error::NotOurBus { address },
    }
}

#[cfg(test)]
mod tests;

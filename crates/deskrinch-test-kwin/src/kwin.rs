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
//! Both refuse with a typed error before any connection. The handle's
//! fields are private, so a question cannot be asked without one, and every
//! connection it makes is to the address it was proved for.

use std::time::Duration;

use deskrinch_test_session::{Environment, KWIN, bus, check};
use zbus::Connection;
use zbus::connection::Builder;

use crate::Error;

/// KWin on a bus of ours: the only way to ask it anything.
#[derive(Debug, Clone)]
pub struct Kwin {
    /// The bus it was proved for.
    address: String,
    /// How long one call may take, when the caller limits it.
    patience: Option<Duration>,
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
        let isolated = check(environment).map_err(Error::NotIsolated)?;
        if isolated.compositor != KWIN {
            return Err(Error::NotKwin {
                compositor: isolated.compositor,
            });
        }
        // The check has read the address and found it a private bus of
        // ours, so it is there and is text.
        let address = environment
            .bus
            .as_ref()
            .and_then(|address| address.to_str())
            .map(str::to_owned)
            .ok_or(Error::NotIsolated(
                deskrinch_test_session::NotIsolated::NoBus,
            ))?;
        Ok(Self::proved(address))
    }

    /// KWin on the bus at `address`, which must be a private bus of ours.
    ///
    /// # Errors
    ///
    /// [`Error::NotOurBus`] for any other address. Nothing is connected to.
    pub fn on(address: &str) -> Result<Self, Error> {
        if bus::is_private(address) {
            Ok(Self::proved(address.to_owned()))
        } else {
            Err(Error::NotOurBus {
                address: address.to_owned(),
            })
        }
    }

    fn proved(address: String) -> Self {
        Self {
            address,
            patience: None,
        }
    }

    /// The same, each call it makes limited to `patience`.
    #[must_use]
    pub fn patience(mut self, patience: Duration) -> Self {
        self.patience = Some(patience);
        self
    }

    /// A connection to its bus, to finish building.
    pub(crate) fn builder(&self) -> Builder<'_> {
        let builder = Builder::address(self.address.as_str());
        match self.patience {
            Some(patience) => builder.method_timeout(patience),
            None => builder,
        }
    }

    /// A connection to its bus.
    pub(crate) async fn connect(&self) -> Result<Connection, Error> {
        Ok(self.builder().build().await?)
    }
}

#[cfg(test)]
mod tests;

//! [`OurBus`]: the proof that a session bus is ours, and the only maker of a
//! connection to one in test code.
//!
//! What must never happen is a test's connection to the owner's bus. There
//! are two ways to know a bus is ours, and they are the proof's only
//! makers:
//!
//! - [`OurBus::here`]: this process runs inside an isolated session of ours
//!   (the session's own check, [`deskrinch_test_session::check`], under any
//!   compositor), and the proof is for that session's bus;
//! - [`OurBus::at`]: the address is a private bus of ours
//!   ([`deskrinch_test_session::bus::is_private`], exact), for a process
//!   that holds the address of a session it started itself.
//!
//! A [`PrivateBus`](crate::PrivateBus) hands out its own
//! ([`PrivateBus::ours`](crate::PrivateBus::ours)), as it made the address.
//! Each maker refuses with [`NotOurs`] before any connection; the proof's
//! fields are private, and every connection it makes is to the address it
//! was proved for.

use std::fmt;
use std::time::Duration;

use deskrinch_test_session::{Environment, Isolated, NotIsolated, bus, check};
use zbus::Connection;
use zbus::connection::Builder;

/// A session bus of ours, proved: the only way test code connects to one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OurBus {
    /// The bus it was proved for.
    address: String,
    /// How long one call may take, when the caller limits it.
    patience: Option<Duration>,
}

impl OurBus {
    /// The bus of the isolated session this process runs in.
    ///
    /// # Errors
    ///
    /// [`NotOurs::NotIsolated`] outside an isolated session of ours.
    /// Nothing is connected to.
    pub fn here() -> Result<Self, NotOurs> {
        Self::in_session(&Environment::read()).map(|(bus, _)| bus)
    }

    /// The bus of the isolated session `environment` describes, and what the
    /// session's check found (its compositor), for a caller that asks more
    /// of the session.
    ///
    /// # Errors
    ///
    /// [`NotOurs::NotIsolated`] when it is not an isolated session of ours.
    pub fn in_session(environment: &Environment) -> Result<(Self, Isolated), NotOurs> {
        let isolated = check(environment).map_err(NotOurs::NotIsolated)?;
        // The check has read the address and found it a private bus of
        // ours, so it is there and is text.
        let address = environment
            .bus
            .as_ref()
            .and_then(|address| address.to_str())
            .ok_or(NotOurs::NotIsolated(NotIsolated::NoBus))?;
        Ok((Self::proved(address.to_owned()), isolated))
    }

    /// The bus at `address`, which must be a private bus of ours.
    ///
    /// # Errors
    ///
    /// [`NotOurs::NotPrivate`] for any other address. Nothing is connected
    /// to.
    pub fn at(address: &str) -> Result<Self, NotOurs> {
        if bus::is_private(address) {
            Ok(Self::proved(address.to_owned()))
        } else {
            Err(NotOurs::NotPrivate {
                address: address.to_owned(),
            })
        }
    }

    /// The proof for `address`, which the caller has proved.
    pub(crate) fn proved(address: String) -> Self {
        Self {
            address,
            patience: None,
        }
    }

    /// The same, each call on its connections limited to `patience`.
    #[must_use]
    pub fn patience(mut self, patience: Duration) -> Self {
        self.patience = Some(patience);
        self
    }

    /// The address it was proved for.
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    /// A connection to it, to finish building (a name to own, objects to
    /// serve).
    pub fn builder(&self) -> Builder<'_> {
        let builder = Builder::address(self.address.as_str());
        match self.patience {
            Some(patience) => builder.method_timeout(patience),
            None => builder,
        }
    }

    /// A connection to it.
    ///
    /// # Errors
    ///
    /// When the bus cannot be reached.
    pub async fn connect(&self) -> zbus::Result<Connection> {
        self.builder().build().await
    }
}

/// Why a bus is not proved ours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotOurs {
    /// The process is not inside an isolated session of ours.
    NotIsolated(NotIsolated),
    /// The address is not a private bus of ours.
    NotPrivate {
        /// The address refused.
        address: String,
    },
}

impl fmt::Display for NotOurs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotIsolated(why) => why.fmt(f),
            Self::NotPrivate { address } => {
                write!(f, "{address} is not a private bus of ours")
            }
        }
    }
}

impl std::error::Error for NotOurs {}

#[cfg(test)]
mod tests;

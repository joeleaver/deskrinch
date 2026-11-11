//! Which D-Bus bus a deskrinch service or client connects to, and the one
//! way to connect to it.
//!
//! A daemon names [`Bus::Session`]; a test names the address of its private
//! bus (`deskrinch-test-dbus`), so nothing it starts touches the real
//! session.
//!
//! # Example
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use deskrinch_bus::Bus;
//!
//! let connection = zbus::block_on(
//!     Bus::Session
//!         .builder()
//!         .method_timeout(Duration::from_secs(5))
//!         .build(),
//! )?;
//! # Ok::<(), zbus::Error>(())
//! ```

use zbus::Connection;
use zbus::connection::Builder;

/// Which bus to connect to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bus {
    /// The session bus, from `DBUS_SESSION_BUS_ADDRESS`.
    Session,
    /// The bus at this D-Bus address, such as `unix:path=/run/…/bus`.
    Address(String),
}

impl Bus {
    /// A builder for a connection to this bus, for a caller that serves
    /// objects on it or sets its own timeouts. An address that is not valid
    /// is reported by the builder's `build`.
    pub fn builder(&self) -> Builder<'_> {
        match self {
            Self::Session => Builder::session(),
            Self::Address(address) => Builder::address(address.as_str()),
        }
    }

    /// Connects to this bus.
    ///
    /// # Errors
    ///
    /// When the address is not valid, or the bus cannot be reached or
    /// authenticated with.
    pub async fn connect(&self) -> zbus::Result<Connection> {
        self.builder().build().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connects_to_the_bus_at_an_address() {
        let bus = deskrinch_test_dbus::PrivateBus::start().expect("needs dbus-daemon");
        let connection = zbus::block_on(Bus::Address(bus.address().to_owned()).connect()).unwrap();
        assert!(connection.unique_name().is_some());
    }

    #[test]
    fn an_address_that_is_not_one_is_an_error() {
        let bus = Bus::Address("not an address".into());
        assert!(zbus::block_on(bus.connect()).is_err());
    }
}

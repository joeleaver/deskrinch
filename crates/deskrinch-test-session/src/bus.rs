//! A session's private bus: the address `deskrinch-test-dbus` gives it and
//! the check that an address is one.
//!
//! The daemon listens on an abstract socket named
//! `/tmp/dbus-deskrinch-<pid>-<n>`; the owner's real session bus is
//! `unix:path=/run/user/<uid>/bus`, and no address of ours is ever that.

/// What every private bus's abstract socket is named with.
const SOCKET_PREFIX: &str = "/tmp/dbus-deskrinch-";

/// The transport and the start of every private bus's address.
const ADDRESS_PREFIX: &str = "unix:abstract=/tmp/dbus-deskrinch-";

/// The abstract socket name of the `n`th private bus process `pid` starts.
#[must_use]
pub fn socket_name(pid: u32, n: usize) -> String {
    format!("{SOCKET_PREFIX}{pid}-{n}")
}

/// The address a daemon listening on the abstract socket `socket` is given.
#[must_use]
pub fn listen_address(socket: &str) -> String {
    format!("unix:abstract={socket}")
}

/// Whether `address` is a private bus of ours: an abstract socket named as
/// [`socket_name`] names one. The daemon adds `,guid=…` to the address it
/// prints, which is allowed.
#[must_use]
pub fn is_private(address: &str) -> bool {
    address
        .strip_prefix(ADDRESS_PREFIX)
        .is_some_and(|rest| !rest.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `cargo xtask session kwin` gave its clients on 2026-11-23 (a
    /// probe's `DBUS_SESSION_BUS_ADDRESS`).
    const SEEN: &str =
        "unix:abstract=/tmp/dbus-deskrinch-4103809-0,guid=9a46e0742c0824b06eb9c2446b03c0af";

    #[test]
    fn a_private_bus_is_ours() {
        assert!(is_private(SEEN));
        assert!(is_private(&listen_address(&socket_name(42, 3))));
        assert_eq!(socket_name(42, 3), "/tmp/dbus-deskrinch-42-3");
    }

    #[test]
    fn the_real_session_bus_and_others_are_not() {
        for address in [
            "unix:path=/run/user/1000/bus",
            "unix:abstract=/tmp/dbus-AbCdEf,guid=0",
            "unix:path=/tmp/dbus-deskrinch-1-0",
            "unix:abstract=/tmp/dbus-deskrinch-",
            "",
        ] {
            assert!(!is_private(address), "{address}");
        }
    }
}

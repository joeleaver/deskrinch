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

/// Whether `address` is a private bus of ours, exactly: an abstract socket
/// named as [`socket_name`] names one (`<pid>-<n>`, both numbers), then
/// nothing, or the `,guid=<hex>` the daemon adds to the address it prints,
/// and nothing after. Nothing else passes: a D-Bus address may be a list,
/// separated by `;` and tried in order, whose later entry could be the real
/// bus, and a key other than `guid` could point elsewhere.
#[must_use]
pub fn is_private(address: &str) -> bool {
    let Some(rest) = address.strip_prefix(ADDRESS_PREFIX) else {
        return false;
    };
    let (name, guid) = match rest.split_once(",guid=") {
        Some((name, guid)) => (name, Some(guid)),
        None => (rest, None),
    };
    let numbers = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let named = name
        .split_once('-')
        .is_some_and(|(pid, n)| numbers(pid) && numbers(n));
    let guided = guid
        .is_none_or(|guid| !guid.is_empty() && guid.bytes().all(|byte| byte.is_ascii_hexdigit()));
    named && guided
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
            // A list, whose second entry is the real bus.
            "unix:abstract=/tmp/dbus-deskrinch-1-0;unix:path=/run/user/1000/bus",
            // A key other than guid.
            "unix:abstract=/tmp/dbus-deskrinch-1-0,path=/run/user/1000/bus",
            // A name not of numbers.
            "unix:abstract=/tmp/dbus-deskrinch-x-0",
            "unix:abstract=/tmp/dbus-deskrinch-1-0x",
            // A guid not of hex digits, and one followed by more.
            "unix:abstract=/tmp/dbus-deskrinch-1-0,guid=xyz",
            "unix:abstract=/tmp/dbus-deskrinch-1-0,guid=9a46;unix:path=/run/user/1000/bus",
        ] {
            assert!(!is_private(address), "{address}");
        }
    }
}

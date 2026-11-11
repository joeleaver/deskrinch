//! The watcher and host on a private bus, driven through the real D-Bus
//! interfaces by stand-in items and by ksni.
//!
//! These tests need `dbus-daemon`; CI installs it. Nothing here touches the
//! session bus: every connection is to the `dbus-daemon` the test starts
//! (`deskrinch-test-dbus`), and the ksni item runs in a child process
//! isolated with that bus as its session bus.

// `cfg(test)` is always on in an integration test; it is spelled out so that
// clippy treats the helpers in these modules as test code, where a failing
// `unwrap` or `expect` is how a broken expectation is reported.
#[cfg(test)]
mod activation;
#[cfg(test)]
mod common;
#[cfg(test)]
mod interop;
#[cfg(test)]
mod lifecycle;
#[cfg(test)]
mod names;
#[cfg(test)]
mod stand_in;
#[cfg(test)]
mod untrusted;

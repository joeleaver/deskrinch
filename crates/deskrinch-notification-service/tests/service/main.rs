//! The service on a private bus, driven through the real D-Bus interface.
//!
//! These tests need `dbus-daemon`, and the interoperability tests
//! `notify-send` (libnotify); CI installs both.

// `cfg(test)` is always on in an integration test; it is spelled out so that
// clippy treats the helpers in these modules as test code, where a failing
// `unwrap` or `expect` is how a broken expectation is reported.
#[cfg(test)]
mod common;
#[cfg(test)]
mod interop;
#[cfg(test)]
mod lifecycle;
#[cfg(test)]
mod names;
#[cfg(test)]
mod pause;
#[cfg(test)]
mod wire;

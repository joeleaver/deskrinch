//! Tests against the real filesystem, processes and a private D-Bus bus.
//!
//! Everything happens in temporary directories: no test reads or writes the
//! user's data directories, launches a real application, or talks to the
//! user's session bus. Launch tests run a small recorder script; D-Bus
//! tests run their own `dbus-daemon` with a configuration that activates
//! nothing.

#[cfg(test)]
mod application;
#[cfg(test)]
mod dbus;
#[cfg(test)]
mod discovery;
#[cfg(test)]
mod launch;
#[cfg(test)]
mod support;
#[cfg(test)]
mod systemd;
#[cfg(test)]
mod watch;

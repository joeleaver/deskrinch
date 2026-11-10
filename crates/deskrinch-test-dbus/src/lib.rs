//! Private D-Bus session buses for tests, isolated from the real session.
//!
//! [`config`] is the one bus configuration deskrinch's tests use: a session
//! bus that serves only what connects to it. [`PrivateBus`] runs a
//! `dbus-daemon` on it for as long as it lives; `cargo xtask
//! compositor-tests` hands the same configuration to `dbus-run-session`.

use std::io::{self, BufRead as _, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The configuration of a private session bus listening on `listen` (a
/// D-Bus server address, such as `unix:dir=/run/…`).
///
/// - No service directories: nothing is activated on demand, so no service
///   of the real desktop (a notification daemon, the portals) can start on
///   the bus.
/// - Everyone connected may own any name, send anything and eavesdrop.
/// - AppArmor mediation of messages is off. Ubuntu confines some clients,
///   `notify-send` among them, with profiles written for the real session
///   bus; on a test bus those rules would only get in the way of the test.
#[must_use]
pub fn config(listen: &str) -> String {
    format!(
        r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>{listen}</listen>
  <auth>EXTERNAL</auth>
  <apparmor mode="disabled"/>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#
    )
}

/// Why a private bus could not start.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The configuration file could not be written.
    #[error("could not write the bus configuration")]
    Config(#[source] io::Error),
    /// `dbus-daemon` could not be started.
    #[error("could not run dbus-daemon (is it installed?)")]
    Spawn(#[source] io::Error),
    /// `dbus-daemon` started but did not print its address.
    #[error("dbus-daemon did not report its address")]
    NoAddress(#[source] Option<io::Error>),
}

/// A `dbus-daemon` of its own, running [`config`]; stopped when dropped.
///
/// It listens on an abstract socket named `/tmp/dbus-deskrinch-…`. Abstract,
/// so nothing is left behind; named like a session bus's, because the
/// AppArmor abstraction `dbus-session-strict` lets a confined client connect
/// only to `@/tmp/dbus-*` and the user's real session bus.
#[derive(Debug)]
pub struct PrivateBus {
    daemon: Child,
    address: String,
    _dir: tempfile::TempDir,
}

impl PrivateBus {
    /// Starts the daemon and waits until it accepts connections.
    ///
    /// # Errors
    ///
    /// When the configuration cannot be written or `dbus-daemon` cannot run.
    pub fn start() -> Result<Self, Error> {
        static BUSES: AtomicUsize = AtomicUsize::new(0);
        let dir = tempfile::tempdir().map_err(Error::Config)?;
        let socket = format!(
            "/tmp/dbus-deskrinch-{}-{}",
            std::process::id(),
            BUSES.fetch_add(1, Ordering::SeqCst)
        );
        let path = dir.path().join("bus.conf");
        std::fs::write(&path, config(&format!("unix:abstract={socket}"))).map_err(Error::Config)?;
        let mut daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", path.display()))
            .args(["--nofork", "--print-address=1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(Error::Spawn)?;
        // The daemon prints its address once it listens.
        let mut address = String::new();
        let read = match daemon.stdout.take() {
            Some(stdout) => BufReader::new(stdout).read_line(&mut address).map_err(Some),
            None => Err(None),
        };
        let mut bus = Self {
            daemon,
            address: address.trim().to_owned(),
            _dir: dir,
        };
        match read {
            Ok(_) if !bus.address.is_empty() => Ok(bus),
            Ok(_) => {
                bus.stop();
                Err(Error::NoAddress(None))
            }
            Err(e) => {
                bus.stop();
                Err(Error::NoAddress(e))
            }
        }
    }

    /// The bus's address, for `DBUS_SESSION_BUS_ADDRESS` or a D-Bus library.
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Stops the daemon now, dropping every connection to it.
    pub fn stop(&mut self) {
        // Either fails only when the daemon has already exited.
        let _killed = self.daemon.kill();
        let _waited = self.daemon.wait();
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_configuration_listens_where_asked_and_activates_nothing() {
        let config = config("unix:dir=/tmp/x");
        assert!(config.contains("<listen>unix:dir=/tmp/x</listen>"));
        assert!(!config.contains("servicedir"));
        assert!(config.contains(r#"<apparmor mode="disabled"/>"#));
    }

    #[test]
    fn a_private_bus_reports_an_abstract_address() {
        let bus = PrivateBus::start().unwrap();
        assert!(
            bus.address()
                .starts_with("unix:abstract=/tmp/dbus-deskrinch-"),
            "{}",
            bus.address()
        );
    }
}

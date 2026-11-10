//! Private D-Bus session buses for tests, isolated from the real session.
//!
//! [`config`] is the one bus configuration deskrinch's tests use: a session
//! bus that serves only what connects to it. [`PrivateBus`] runs a
//! `dbus-daemon` on it, in an isolated session ([`session`]) and a process
//! group of its own, for as long as it lives; `cargo xtask
//! compositor-tests` hands the same configuration and isolation to the
//! sessions it starts.
//!
//! A test of D-Bus activation itself opts in to service directories of its
//! own with [`config_with_services`] or [`PrivateBus::start_with_services`].
//! The services the bus starts are the daemon's children: they run in its
//! isolated session and its process group, and end with it.

pub mod session;

use std::ffi::{OsStr, OsString};
use std::io::{self, BufRead as _, BufReader};
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use rustix::process::{Pid, Signal, kill_process_group};

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
    config_with_services(listen, &[])
}

/// [`config`], except that the bus activates services from the D-Bus
/// service files in `service_dirs`, and only from there. Off by default: on
/// a test bus, activation from the system's directories is what would start
/// the real desktop's portals and daemons.
#[must_use]
pub fn config_with_services(listen: &str, service_dirs: &[&Path]) -> String {
    let service_dirs: String = service_dirs
        .iter()
        .map(|dir| {
            [
                "  <servicedir>",
                &dir.display().to_string(),
                "</servicedir>\n",
            ]
            .concat()
        })
        .collect();
    format!(
        r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>{listen}</listen>
{service_dirs}  <auth>EXTERNAL</auth>
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
    /// The session's directories or the configuration file could not be
    /// written.
    #[error("could not set up the bus's session directory")]
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
///
/// The daemon runs isolated ([`session::isolate`]) under a temporary root,
/// in a process group of its own that [`stop`](Self::stop) ends as a whole,
/// services it activated included. Programs a test runs against the bus
/// come from [`command`](Self::command), isolated the same way.
#[derive(Debug)]
pub struct PrivateBus {
    daemon: Child,
    address: String,
    dir: tempfile::TempDir,
}

impl PrivateBus {
    /// Starts the daemon and waits until it accepts connections.
    ///
    /// # Errors
    ///
    /// When the session cannot be set up or `dbus-daemon` cannot run.
    pub fn start() -> Result<Self, Error> {
        Self::start_with_services(&[])
    }

    /// Starts a daemon that activates services from `service_dirs`; see
    /// [`config_with_services`].
    ///
    /// # Errors
    ///
    /// When the session cannot be set up or `dbus-daemon` cannot run.
    pub fn start_with_services(service_dirs: &[&Path]) -> Result<Self, Error> {
        static BUSES: AtomicUsize = AtomicUsize::new(0);
        let dir = tempfile::tempdir().map_err(Error::Config)?;
        session::create_dirs(dir.path()).map_err(Error::Config)?;
        let socket = format!(
            "/tmp/dbus-deskrinch-{}-{}",
            std::process::id(),
            BUSES.fetch_add(1, Ordering::SeqCst)
        );
        let path = dir.path().join("bus.conf");
        std::fs::write(
            &path,
            config_with_services(&format!("unix:abstract={socket}"), service_dirs),
        )
        .map_err(Error::Config)?;
        let mut command = Command::new("dbus-daemon");
        session::isolate(&mut command, dir.path());
        let mut daemon = command
            .arg(format!("--config-file={}", path.display()))
            .args(["--nofork", "--print-address=1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .process_group(0)
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
            dir,
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

    /// The root of the bus's isolated session (its home and XDG directories
    /// are [`session::SESSION_DIRS`] under it).
    #[must_use]
    pub fn session_root(&self) -> &Path {
        self.dir.path()
    }

    /// A command for `program` that runs in this bus's isolated session,
    /// with `DBUS_SESSION_BUS_ADDRESS` naming this bus.
    #[must_use]
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(program);
        session::isolate(&mut command, self.dir.path());
        command.env("DBUS_SESSION_BUS_ADDRESS", &self.address);
        command
    }

    /// The variables [`command`](Self::command) sets: the session's
    /// [`session::environment`] and `DBUS_SESSION_BUS_ADDRESS`. For code
    /// under test that builds its own commands and takes extra variables
    /// rather than a [`Command`]; such a command still inherits the real
    /// session's variables ([`session::REAL_SESSION_VARIABLES`]) unless the
    /// code under test removes them.
    #[must_use]
    pub fn session_env(&self) -> Vec<(&'static str, OsString)> {
        session::environment(self.dir.path())
            .into_iter()
            .chain([("DBUS_SESSION_BUS_ADDRESS", OsString::from(&self.address))])
            .collect()
    }

    /// Stops the daemon's process group (the daemon and the services it
    /// activated) now, dropping every connection to the bus.
    pub fn stop(&mut self) {
        // The group may already be gone; either way it is asked to end.
        let _killed = kill_process_group(Pid::from_child(&self.daemon), Signal::KILL);
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
    fn service_directories_are_opt_in() {
        let config = config_with_services("unix:dir=/tmp/x", &[Path::new("/tmp/services")]);
        assert!(config.contains("<servicedir>/tmp/services</servicedir>"));
        assert_eq!(config.matches("servicedir>").count(), 2);
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

    #[test]
    fn commands_run_in_the_bus_session() {
        let bus = PrivateBus::start().unwrap();
        let command = bus.command("true");
        let envs: Vec<_> = command.get_envs().collect();
        let runtime = bus.session_root().join("run");
        assert!(envs.contains(&("XDG_RUNTIME_DIR".as_ref(), Some(runtime.as_os_str()))));
        assert!(envs.contains(&(
            "DBUS_SESSION_BUS_ADDRESS".as_ref(),
            Some(OsStr::new(bus.address()))
        )));
        assert!(envs.contains(&("WAYLAND_DISPLAY".as_ref(), None)));
    }
}

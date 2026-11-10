//! A nested, headless compositor session, isolated from the real one.
//!
//! What a session is, which [`Session::client`] commands share:
//!
//! - The explicit environment of `deskrinch-test-dbus`'s
//!   [`session::isolate`]: its own `HOME` and XDG directories, private
//!   directories under a fresh `/tmp/deskrinch-session-*` root, the XDG
//!   search paths at their defaults, and none of the real session's display,
//!   bus or login-session variables ([`session::REAL_SESSION_VARIABLES`]);
//!   `XDG_CURRENT_DESKTOP` only when the caller asks for one.
//! - A private D-Bus session bus ([`PrivateBus`]) with no service activation,
//!   named in `DBUS_SESSION_BUS_ADDRESS` for the compositor and every client.
//! - The compositor, headless, and every client in one process group of
//!   their own, which is ended as a whole (`SIGTERM`, then `SIGKILL` after
//!   [`SHUTDOWN`]) when the session ends, and also when xtask gets `SIGINT`,
//!   `SIGTERM` or `SIGHUP` ([`interrupt`]).
//! - Afterwards, a check that nothing of the session runs any more: a
//!   process that left the group is found by its environment, killed, and
//!   reported as an error ([`leftovers`]).
//!
//! The isolation itself (directories, variables, the bus) is
//! `deskrinch-test-dbus`'s, which the D-Bus tests use too.

mod compositor;
pub(crate) mod interrupt;
mod leftovers;

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_test_dbus::PrivateBus;
use deskrinch_test_dbus::session::{self, isolate};
use rustix::process::{
    Pid, Signal, kill_process, kill_process_group, test_kill_process, test_kill_process_group,
};
use tempfile::TempDir;

pub(crate) use self::compositor::Compositor;
pub(crate) use self::leftovers::Leftover;
use crate::error::Error;

/// The variable naming the desktop that runs, which the session leaves
/// unset unless asked.
const CURRENT_DESKTOP: &str = "XDG_CURRENT_DESKTOP";

/// How long a compositor has to open its socket.
const STARTUP: Duration = Duration::from_secs(20);
/// How long a session has to end after `SIGTERM` before it gets `SIGKILL`.
const SHUTDOWN: Duration = Duration::from_secs(5);
/// How often to look again while waiting.
const POLL: Duration = Duration::from_millis(50);

/// A running nested compositor.
#[derive(Debug)]
pub(crate) struct Session {
    compositor: Compositor,
    /// The session's root; `None` once it has been kept or removed.
    dir: Option<TempDir>,
    root: PathBuf,
    bus: PrivateBus,
    child: Child,
    group: Pid,
    socket: String,
    current_desktop: Option<OsString>,
    keep: bool,
    ended: bool,
}

impl Session {
    /// Starts `compositor` with `outputs` outputs in a new isolated session,
    /// after `prepare` has set up the session's data directory
    /// (`XDG_DATA_HOME`).
    pub(crate) fn start(
        compositor: Compositor,
        outputs: u32,
        current_desktop: Option<&OsStr>,
        prepare: impl FnOnce(&Path) -> Result<(), Error>,
    ) -> Result<Self, Error> {
        interrupt::install()?;
        interrupt::check()?;
        // In `/tmp`, not `$TMPDIR`: the Wayland socket's path must fit a Unix
        // socket address (108 bytes), and `$TMPDIR` may be long.
        let dir = tempfile::Builder::new()
            .prefix("deskrinch-session-")
            .tempdir_in("/tmp")
            .map_err(|source| Error::Io {
                what: "create the session directory".into(),
                source,
            })?;
        let root = dir.path().to_owned();
        session::create_dirs(&root).map_err(|source| Error::Io {
            what: "create the session's directories".into(),
            source,
        })?;
        prepare(&root.join("data"))?;
        let bus = PrivateBus::start()?;

        let log_path = root.join("compositor.log");
        let log = File::create(&log_path).map_err(|source| Error::Io {
            what: format!("create {}", log_path.display()),
            source,
        })?;
        let log_err = log.try_clone().map_err(|source| Error::Io {
            what: "share the compositor log".into(),
            source,
        })?;

        let mut command = compositor.command(&root, outputs)?;
        isolate(&mut command, &root);
        if let Some(desktop) = current_desktop {
            command.env(CURRENT_DESKTOP, desktop);
        }
        command
            .env("DBUS_SESSION_BUS_ADDRESS", bus.address())
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log_err)
            // A process group of its own, so the session ends as a whole.
            .process_group(0);
        let program = command.get_program().to_string_lossy().into_owned();
        let child = command
            .spawn()
            .map_err(|source| Error::Spawn { program, source })?;
        let group = Pid::from_child(&child);
        interrupt::watch(group);

        let mut session = Self {
            compositor,
            dir: Some(dir),
            root,
            bus,
            child,
            group,
            socket: String::new(),
            current_desktop: current_desktop.map(ToOwned::to_owned),
            keep: false,
            ended: false,
        };
        session.socket = session.wait_for_socket()?;
        Ok(session)
    }

    /// Keeps the session's directory (its logs, and whatever clients wrote
    /// there) when it ends, instead of removing it.
    pub(crate) fn keep(&mut self) {
        self.keep = true;
    }

    /// The session's root directory, which clients see as
    /// `DESKRINCH_SESSION_DIR`.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Runs a test binary's ignored tests in the session, one at a time,
    /// with `filter` (libtest arguments) choosing which. Returns whether they
    /// passed.
    ///
    /// The tests are told the compositor and the session's process group, so
    /// that one can end the session underneath a connection.
    pub(crate) fn run_tests(&self, binary: &Path, filter: &[&str]) -> Result<bool, Error> {
        let mut command = self.outside_client(binary.as_os_str());
        command
            .args(["--ignored", "--test-threads=1"])
            .args(filter)
            .env("DESKRINCH_TEST_COMPOSITOR", self.compositor.name())
            .env(
                "DESKRINCH_TEST_SESSION_GROUP",
                self.group.as_raw_nonzero().to_string(),
            );
        let status = command.status().map_err(|source| Error::Spawn {
            program: binary.display().to_string(),
            source,
        })?;
        interrupt::check()?;
        Ok(status.success())
    }

    /// A command that runs `program` as a client of the session: isolated
    /// from the real session; pointed at this compositor
    /// (`WAYLAND_DISPLAY`, and sway's `SWAYSOCK`) and at the session's bus
    /// (`DBUS_SESSION_BUS_ADDRESS`); told the session's directory
    /// (`DESKRINCH_SESSION_DIR`); with standard input closed; and in the
    /// session's process group, so that whatever it leaves running ends with
    /// the session.
    ///
    /// Standard input is closed because the session's group is not the
    /// terminal's foreground group: a client reading the terminal would be
    /// stopped (`SIGTTIN`) and hang the session.
    pub(crate) fn client(&self, program: &OsStr) -> Command {
        let mut command = self.outside_client(program);
        command.process_group(self.group.as_raw_nonzero().get());
        command
    }

    /// [`client`](Self::client) without joining the session's process
    /// group: for a test runner whose tests end the session by signalling
    /// that group, and must not end themselves with it. What it leaves
    /// running is still found by the check for leftovers.
    fn outside_client(&self, program: &OsStr) -> Command {
        let mut command = Command::new(program);
        isolate(&mut command, &self.root);
        command
            .env("WAYLAND_DISPLAY", &self.socket)
            .env("DBUS_SESSION_BUS_ADDRESS", self.bus.address())
            .env("DESKRINCH_SESSION_DIR", &self.root)
            .envs(self.compositor.client_env(&self.root.join("run")))
            .stdin(Stdio::null());
        if let Some(desktop) = &self.current_desktop {
            command.env(CURRENT_DESKTOP, desktop);
        }
        command
    }

    /// Whether the compositor is still running.
    pub(crate) fn compositor_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// The compositor's output so far.
    pub(crate) fn log(&self) -> String {
        fs::read_to_string(self.root.join("compositor.log")).unwrap_or_default()
    }

    /// Ends the session and checks that nothing of it is left. Leftovers
    /// are killed and reported, and the session's directory is then kept.
    pub(crate) fn end(mut self) -> Result<(), Error> {
        self.finish()
    }

    fn finish(&mut self) -> Result<(), Error> {
        if self.ended {
            return Ok(());
        }
        self.ended = true;
        let group_left = self.stop_group();
        interrupt::forget(self.group);
        self.bus.stop();
        let mut leftovers = leftovers::find(&self.root.join("run"));
        for leftover in &leftovers {
            // Already gone, or not ours to signal: either way not running on.
            let _ = kill_process(leftover.pid, Signal::KILL);
        }
        // `SIGKILL` takes effect asynchronously; return once they are gone.
        let deadline = Instant::now() + SHUTDOWN;
        while leftovers
            .iter()
            .any(|leftover| test_kill_process(leftover.pid).is_ok())
            && Instant::now() < deadline
        {
            thread::sleep(POLL);
        }
        if group_left {
            leftovers.insert(
                0,
                Leftover {
                    pid: self.group,
                    command: "(the session's process group)".into(),
                },
            );
        }
        let keep = self.keep || !leftovers.is_empty();
        if let Some(dir) = self.dir.take() {
            if keep {
                let kept = dir.keep();
                eprintln!("session: kept {}", kept.display());
            } else {
                dir.close().map_err(|source| Error::Io {
                    what: format!("remove {}", self.root.display()),
                    source,
                })?;
            }
        }
        if leftovers.is_empty() {
            Ok(())
        } else {
            Err(Error::SessionLeftovers {
                leftovers,
                root: self.root.clone(),
            })
        }
    }

    /// Waits for the compositor's Wayland socket and returns its name.
    fn wait_for_socket(&mut self) -> Result<String, Error> {
        let runtime = self.root.join("run");
        let deadline = Instant::now() + STARTUP;
        loop {
            interrupt::check()?;
            if let Some(socket) = compositor::wayland_socket(&runtime) {
                return Ok(socket);
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(Error::CompositorExited {
                    compositor: self.compositor.name(),
                    status,
                    log: self.log(),
                });
            }
            if Instant::now() >= deadline {
                return Err(Error::CompositorTimedOut {
                    compositor: self.compositor.name(),
                    log: self.log(),
                });
            }
            thread::sleep(POLL);
        }
    }

    /// Ends every process of the session's group: `SIGTERM`, then `SIGKILL`
    /// for what is left after [`SHUTDOWN`]. Returns whether any of the group
    /// outlived even that.
    fn stop_group(&mut self) -> bool {
        // The group may already be gone; either way it is asked to end.
        let _ = kill_process_group(self.group, Signal::TERM);
        let deadline = Instant::now() + SHUTDOWN;
        loop {
            // Reap the leader, so that its zombie does not keep the group.
            drop(self.child.try_wait());
            if test_kill_process_group(self.group).is_err() {
                return false;
            }
            if Instant::now() >= deadline {
                let _ = kill_process_group(self.group, Signal::KILL);
                drop(self.child.wait());
                thread::sleep(POLL);
                return test_kill_process_group(self.group).is_ok();
            }
            thread::sleep(POLL);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A session dropped on an error path still ends; its own failure to
        // end cleanly can only be reported.
        if let Err(error) = self.finish() {
            eprintln!("session: {error}");
        }
    }
}

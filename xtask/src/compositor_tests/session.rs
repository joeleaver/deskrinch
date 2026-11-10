//! A nested, headless compositor session, isolated from the real one.
//!
//! The session has its own runtime, data, configuration, cache and state
//! directories in a temporary directory, its own D-Bus session bus (KWin)
//! with service activation turned off, and runs in a process group of its
//! own, which is ended as a whole when the session is dropped. Nothing it
//! starts can reach the real session's sockets, bus or files.

use std::fs::{self, File};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_test_dbus::session::{self, isolate};
use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};
use tempfile::TempDir;

use super::Compositor;
use crate::error::Error;

/// How long a compositor has to open its socket.
const STARTUP: Duration = Duration::from_secs(20);
/// How long a session has to end after `SIGTERM` before it gets `SIGKILL`.
const SHUTDOWN: Duration = Duration::from_secs(5);
/// How often to look again while waiting.
const POLL: Duration = Duration::from_millis(50);

/// The KWin socket's name; the wlroots compositors pick their own.
const KWIN_SOCKET: &str = "wayland-0";
/// How many outputs the session has, side by side.
const OUTPUTS: u32 = 2;
/// Each output's size, where the compositor takes one.
const OUTPUT_SIZE: (u32, u32) = (1280, 720);

/// A running nested compositor.
#[derive(Debug)]
pub(super) struct Session {
    compositor: Compositor,
    dir: TempDir,
    child: Child,
    socket: String,
}

impl Session {
    /// Starts `compositor` in a new isolated session, after `prepare` has
    /// set up the session's data directory (`XDG_DATA_HOME`).
    pub(super) fn start(
        compositor: Compositor,
        prepare: impl FnOnce(&Path) -> Result<(), Error>,
    ) -> Result<Self, Error> {
        // In `/tmp`, not `$TMPDIR`: the Wayland socket's path must fit a Unix
        // socket address (108 bytes), and `$TMPDIR` may be long.
        let dir = tempfile::Builder::new()
            .prefix("deskrinch-session-")
            .tempdir_in("/tmp")
            .map_err(|source| Error::Io {
                what: "create the session directory".into(),
                source,
            })?;
        session::create_dirs(dir.path()).map_err(|source| Error::Io {
            what: "create the session's directories".into(),
            source,
        })?;
        prepare(&dir.path().join("data"))?;

        let log_path = dir.path().join("compositor.log");
        let log = File::create(&log_path).map_err(|source| Error::Io {
            what: format!("create {}", log_path.display()),
            source,
        })?;
        let log_err = log.try_clone().map_err(|source| Error::Io {
            what: "share the compositor log".into(),
            source,
        })?;

        let mut command = compositor_command(compositor, dir.path())?;
        isolate(&mut command, dir.path());
        command
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log_err)
            // A process group of its own, so the session ends as a whole.
            .process_group(0);
        let program = command.get_program().to_string_lossy().into_owned();
        let child = command
            .spawn()
            .map_err(|source| Error::Spawn { program, source })?;

        let mut session = Self {
            compositor,
            dir,
            child,
            socket: String::new(),
        };
        session.socket = session.wait_for_socket()?;
        Ok(session)
    }

    /// Runs a test binary's ignored tests in the session, one at a time,
    /// with `filter` (libtest arguments) choosing which. Returns whether they
    /// passed.
    ///
    /// The tests are told the session's process group, so that one can end
    /// the session underneath a connection.
    pub(super) fn run_tests(&self, binary: &Path, filter: &[&str]) -> Result<bool, Error> {
        let mut command = Command::new(binary);
        isolate(&mut command, self.dir.path());
        command
            .args(["--ignored", "--test-threads=1"])
            .args(filter)
            .env("DESKRINCH_TEST_SESSION_GROUP", self.child.id().to_string())
            .env("WAYLAND_DISPLAY", &self.socket)
            .env("DESKRINCH_TEST_COMPOSITOR", self.compositor.name());
        let status = command.status().map_err(|source| Error::Spawn {
            program: binary.display().to_string(),
            source,
        })?;
        Ok(status.success())
    }

    /// The compositor's output so far.
    pub(super) fn log(&self) -> String {
        fs::read_to_string(self.dir.path().join("compositor.log")).unwrap_or_default()
    }

    /// Waits for the compositor's Wayland socket and returns its name.
    fn wait_for_socket(&mut self) -> Result<String, Error> {
        let runtime = self.dir.path().join("run");
        let deadline = Instant::now() + STARTUP;
        loop {
            if let Some(socket) = wayland_socket(&runtime) {
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

    /// Ends every process of the session: `SIGTERM`, then `SIGKILL` for what
    /// is left after [`SHUTDOWN`].
    fn stop(&mut self) {
        let group = Pid::from_child(&self.child);
        // The group may already be gone; either way it is asked to end.
        let _ = kill_process_group(group, Signal::TERM);
        let deadline = Instant::now() + SHUTDOWN;
        loop {
            // Reap the leader, so that its zombie does not keep the group.
            drop(self.child.try_wait());
            if test_kill_process_group(group).is_err() {
                return;
            }
            if Instant::now() >= deadline {
                let _ = kill_process_group(group, Signal::KILL);
                drop(self.child.wait());
                return;
            }
            thread::sleep(POLL);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The command that starts `compositor` with its session directory `dir`.
fn compositor_command(compositor: Compositor, dir: &Path) -> Result<Command, Error> {
    match compositor {
        Compositor::Kwin => {
            let runtime = dir.join("run");
            let config = dir.join("dbus.conf");
            // A session bus that serves only what connects to it: no service
            // activation, so nothing (such as the desktop portals, whose
            // document portal mounts a FUSE file system in the runtime
            // directory) is started on demand.
            let contents =
                deskrinch_test_dbus::config(&format!("unix:dir={}", runtime.to_string_lossy()));
            fs::write(&config, contents).map_err(|source| Error::Io {
                what: format!("write {}", config.display()),
                source,
            })?;
            let mut command = Command::new("dbus-run-session");
            command
                .arg(format!("--config-file={}", config.display()))
                .args(["--", "kwin_wayland", "--virtual", "--socket", KWIN_SOCKET])
                .args([
                    "--no-lockscreen",
                    "--no-global-shortcuts",
                    "--no-kactivities",
                ])
                .arg(format!("--output-count={OUTPUTS}"))
                .arg(format!("--width={}", OUTPUT_SIZE.0))
                .arg(format!("--height={}", OUTPUT_SIZE.1));
            Ok(command)
        }
        Compositor::Sway => {
            // An empty configuration: no bar, background or key bindings.
            let config = dir.join("sway.conf");
            fs::write(&config, "").map_err(|source| Error::Io {
                what: format!("write {}", config.display()),
                source,
            })?;
            let mut command = Command::new("sway");
            command.arg("--config").arg(&config);
            wlroots_headless(&mut command);
            Ok(command)
        }
        Compositor::Labwc => {
            // An empty configuration directory: labwc's defaults, and no
            // autostart.
            let config = dir.join("labwc");
            session::create_private_dir(&config).map_err(|source| Error::Io {
                what: format!("create {}", config.display()),
                source,
            })?;
            let mut command = Command::new("labwc");
            command
                .arg("--config-dir")
                .arg(&config)
                // It would only update it on a DRM backend, and without a
                // session bus it cannot; said here so that it never does.
                .env("LABWC_UPDATE_ACTIVATION_ENV", "false");
            wlroots_headless(&mut command);
            Ok(command)
        }
    }
}

/// Runs a wlroots compositor on the headless backend, with software
/// rendering, no input devices and [`OUTPUTS`] outputs.
fn wlroots_headless(command: &mut Command) {
    command
        .env("WLR_BACKENDS", "headless")
        .env("WLR_RENDERER", "pixman")
        .env("WLR_HEADLESS_OUTPUTS", OUTPUTS.to_string())
        .env("WLR_LIBINPUT_NO_DEVICES", "1");
}

/// The name of the Wayland socket in `runtime`, once there is one.
fn wayland_socket(runtime: &Path) -> Option<String> {
    fs::read_dir(runtime)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_socket()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .find(|name| is_wayland_socket_name(name))
}

/// Whether `name` is a compositor's `wayland-*` socket (not its lock file,
/// and not another program's socket).
fn is_wayland_socket_name(name: &str) -> bool {
    name.strip_prefix("wayland-")
        .is_some_and(|rest| !rest.is_empty() && !rest.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_wayland_sockets() {
        assert!(is_wayland_socket_name("wayland-0"));
        assert!(is_wayland_socket_name("wayland-1"));
        assert!(!is_wayland_socket_name("wayland-0.lock"));
        assert!(!is_wayland_socket_name("wayland-"));
        assert!(!is_wayland_socket_name("sway-ipc.1000.1.sock"));
    }

    #[test]
    fn finds_only_a_socket() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        fs::write(dir.path().join("wayland-0"), "").unwrap();
        assert_eq!(wayland_socket(dir.path()), None, "a plain file is not one");
        fs::remove_file(dir.path().join("wayland-0")).unwrap();
        let _listener =
            std::os::unix::net::UnixListener::bind(dir.path().join("wayland-0")).unwrap();
        assert_eq!(wayland_socket(dir.path()).as_deref(), Some("wayland-0"));
    }
}

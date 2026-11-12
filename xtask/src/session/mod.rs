//! A nested compositor session, isolated from the real one: headless, or
//! with its outputs in windows on another compositor ([`Display`]).
//!
//! What a session is, which [`Session::client`] commands share:
//!
//! - The explicit environment of `deskrinch-test-dbus`'s [`isolate`]: its
//!   own `HOME` and XDG directories, private directories under a fresh
//!   `/tmp/deskrinch-session-*` root, the XDG search paths at their
//!   defaults, and none of the real session's display, bus or login-session
//!   variables ([`REAL_SESSION_VARIABLES`]); `XDG_CURRENT_DESKTOP` only when
//!   the caller asks for one.
//! - A private D-Bus session bus ([`PrivateBus`]) with no service activation,
//!   named in `DBUS_SESSION_BUS_ADDRESS` for the compositor and every client.
//! - Ready when the compositor's Wayland socket is open and it owns the
//!   D-Bus names it registers at startup ([`Compositor::bus_names`]: KWin's
//!   services; none for sway and labwc), each waited for within
//!   [`STARTUP`].
//! - For KWin, also: `CI` removed from KWin's environment (KWin's virtual
//!   backend takes it to mean KWin's own CI and then looks only at
//!   `/dev/dri/card1`; GitHub's runners set it), and KWin's own statement of
//!   its compositing type checked, failing the session unless it is OpenGL:
//!   with `QPainter` KWin starts, but its `ScreenShot2` captures nothing
//!   ([`compositing`]).
//! - Its outputs at the scale asked for before the first client connects
//!   ([`Scaling`]): sway through its configuration, labwc set through
//!   `wlr-output-management`, KWin set with `kscreen-doctor` (which runs on
//!   Qt's Wayland platform and needs its xdg-shell integration,
//!   `libqt6wlshellintegration6` on Debian and Ubuntu); then checked against
//!   the scale and logical size each output reports.
//! - The compositor, headless, and every client in one process group of
//!   their own, which is ended as a whole (`SIGTERM`, then `SIGKILL` after
//!   [`ending::SHUTDOWN`], then waited for until it is gone, [`ending`])
//!   when the session ends, and also when xtask gets `SIGINT`, `SIGTERM` or
//!   `SIGHUP` ([`interrupt`]).
//! - Afterwards, a check that nothing of the session runs any more: a
//!   process that left the group is found by its environment, killed, and
//!   reported as an error ([`leftovers`]).
//! - A supervisor, started before anything of the session: should xtask
//!   die without ending the session (`SIGKILL`, a crash), it ends the
//!   group, the processes that left it, and the directory ([`supervisor`]).
//!   A session whose supervisor cannot be started does not start.
//!
//! The isolation itself (directories, variables, the bus) is
//! `deskrinch-test-dbus`'s, which the D-Bus tests use too.
//!
//! [`REAL_SESSION_VARIABLES`]: deskrinch_test_dbus::session::REAL_SESSION_VARIABLES

mod clients;
mod compositing;
mod compositor;
mod display;
mod ending;
pub(crate) mod interrupt;
mod leftovers;
mod outputs;
mod render_nodes;
mod rendering;
mod scale;
mod scaling;
mod supervisor;
mod transform;

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_test_dbus::session::isolate;
use deskrinch_test_dbus::{NameError, PrivateBus};
use rustix::process::Pid;
use tempfile::TempDir;

use self::ending::{end_group, kill_leftovers};

pub(crate) use self::clients::ClientId;
pub(crate) use self::compositor::Compositor;
pub(crate) use self::display::{Binding, Display, Home, Options};
pub(crate) use self::leftovers::Leftover;
pub(crate) use self::outputs::Outputs;
pub(crate) use self::render_nodes::RenderNodes;
pub(crate) use self::rendering::CompositorRenderer;
pub(crate) use self::scale::Scale;
pub(crate) use self::scaling::Scaling;
use self::supervisor::Supervisor;
#[cfg(test)]
pub(crate) use self::supervisor::fake_root;
pub(crate) use self::supervisor::{
    SUBCOMMAND as SUPERVISOR, check as check_root, run as supervise, supervised,
};
pub(crate) use self::transform::OutputTransform;
use crate::error::Error;

/// The variable naming the desktop that runs, which the session leaves
/// unset unless asked.
const CURRENT_DESKTOP: &str = "XDG_CURRENT_DESKTOP";

/// Where a session's root is made: `/tmp`, not `$TMPDIR`, as the Wayland
/// socket's path must fit a Unix socket address (108 bytes), and `$TMPDIR`
/// may be long.
const ROOT_PARENT: &str = "/tmp";
/// What a session's root is named, before tempfile's random part.
const ROOT_PREFIX: &str = "deskrinch-session-";

/// How long a compositor has to open its socket.
const STARTUP: Duration = Duration::from_secs(20);
/// How often to look again while waiting.
const POLL: Duration = Duration::from_millis(50);

/// A running nested compositor.
#[derive(Debug)]
pub(crate) struct Session {
    compositor: Compositor,
    renderer: CompositorRenderer,
    display: Display,
    /// The session's root; `None` once it has been kept or removed.
    dir: Option<TempDir>,
    root: PathBuf,
    bus: PrivateBus,
    child: Child,
    /// The clients started with [`spawn`](Self::spawn), reaped as the
    /// session ends.
    clients: Vec<Child>,
    group: Pid,
    socket: String,
    current_desktop: Option<OsString>,
    keep: bool,
    ended: bool,
    /// Ends the session should xtask die without ending it; released once
    /// the session has ended.
    supervisor: Option<Supervisor>,
}

impl Session {
    /// Starts `compositor` rendering with `renderer`, with `outputs`, in a
    /// new isolated, headless session ([`Options::headless`]), after
    /// `prepare` has set up the session's data directory (`XDG_DATA_HOME`).
    pub(crate) fn start(
        compositor: Compositor,
        renderer: CompositorRenderer,
        outputs: Outputs,
        current_desktop: Option<&OsStr>,
        prepare: impl FnOnce(&Path) -> Result<(), Error>,
    ) -> Result<Self, Error> {
        let options = Options::headless(compositor, renderer, outputs, current_desktop);
        Self::start_with(&options, prepare)
    }

    /// Starts a new isolated session as `options` say, after `prepare` has
    /// set up the session's data directory (`XDG_DATA_HOME`).
    pub(crate) fn start_with(
        options: &Options<'_>,
        prepare: impl FnOnce(&Path) -> Result<(), Error>,
    ) -> Result<Self, Error> {
        let Options {
            compositor,
            renderer,
            outputs,
            current_desktop,
            ref display,
            ref home,
            bindings: _,
        } = *options;
        interrupt::install()?;
        interrupt::check()?;
        let dir = tempfile::Builder::new()
            .prefix(ROOT_PREFIX)
            .tempdir_in(ROOT_PARENT)
            .map_err(|source| Error::Io {
                what: "create the session directory".into(),
                source,
            })?;
        let root = dir.path().to_owned();
        home.create_dirs(&root).map_err(|source| Error::Io {
            what: "create the session's directories".into(),
            source,
        })?;
        // Before anything of the session runs: a session that could not be
        // supervised does not start.
        let mut supervisor = Supervisor::start(&root)?;
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

        let mut command = compositor.command(&root, options)?;
        isolate(&mut command, &root);
        compositor.show_on(&mut command, display);
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
        let told = supervisor.group(group);

        let mut session = Self {
            compositor,
            renderer,
            display: display.clone(),
            dir: Some(dir),
            root,
            bus,
            child,
            clients: Vec::new(),
            group,
            socket: String::new(),
            current_desktop: current_desktop.map(ToOwned::to_owned),
            keep: false,
            ended: false,
            supervisor: Some(supervisor),
        };
        // A supervisor that cannot be told the group cannot end it: the
        // session ends now (dropped), and does not start.
        told?;
        session.socket = session.wait_for_socket()?;
        session.wait_for_bus_names()?;
        session.check_wlroots_renderer()?;
        session.set_up_outputs(outputs)?;
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

    /// The session's process group: the compositor and its clients.
    pub(crate) fn group(&self) -> Pid {
        self.group
    }

    /// How the compositor ended, once it has; `None` while it runs.
    pub(crate) fn compositor_status(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
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
        let (child, clients) = (&mut self.child, &mut self.clients);
        let group_left = end_group(self.group, || {
            drop(child.try_wait());
            clients::reap(clients);
        });
        interrupt::forget(self.group);
        self.bus.stop();
        let mut leftovers = kill_leftovers(&self.root.join("run"));
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
        let removed = match self.dir.take() {
            Some(dir) if keep => {
                let kept = dir.keep();
                eprintln!("session: kept {}", kept.display());
                Ok(())
            }
            Some(dir) => dir.close().map_err(|source| Error::Io {
                what: format!("remove {}", self.root.display()),
                source,
            }),
            None => Ok(()),
        };
        // Ended, whatever came of the directory: the supervisor leaves.
        if let Some(supervisor) = self.supervisor.take() {
            supervisor.release();
        }
        removed?;
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

    /// Waits until the compositor owns each of its [`Compositor::bus_names`].
    fn wait_for_bus_names(&self) -> Result<(), Error> {
        for &name in self.compositor.bus_names() {
            self.bus
                .wait_for_name(name, STARTUP)
                .map_err(|source| Error::CompositorNotReady {
                    compositor: self.compositor.name(),
                    name,
                    hint: self.compositor.missing_name_hint(name),
                    source: Box::new(source),
                    log: self.log(),
                })?;
            interrupt::check()?;
        }
        if self.compositor.checks_opengl(&self.display) {
            self.check_kwin_compositing(STARTUP)?;
        }
        Ok(())
    }

    /// Waits up to `patience` until `name` is owned on the session's bus.
    pub(crate) fn wait_for_name(&self, name: &str, patience: Duration) -> Result<(), NameError> {
        self.bus.wait_for_name(name, patience)
    }

    /// The session's bus address.
    pub(crate) fn bus_address(&self) -> &str {
        self.bus.address()
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

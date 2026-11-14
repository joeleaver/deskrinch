//! What the tests share: the launcher binary run and ended, the bus asked
//! who owns the launcher's name, and waiting.

use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use deskrinch_bus::Bus;
use deskrinch_launcher::NAME;
use deskrinch_test_client::Compositor;
use deskrinch_test_component::{Component, Renderer};
use deskrinch_test_wait::PATIENCE;

/// How long a test watches for something that must not happen.
pub(crate) const QUIET: Duration = Duration::from_secs(2);

pub(crate) const LAUNCHER: &str = env!("CARGO_BIN_EXE_deskrinch-launcher");

/// Panics unless the tests run in a nested session started by
/// `cargo xtask compositor-tests` (`Compositor::current`). Run anywhere
/// else, they would take the launcher's name on the real session bus, map
/// an overlay with the keyboard on the real compositor, and type into it.
/// Every test calls it first, before it touches the bus, the screen or the
/// keyboard, or starts a process.
pub(crate) fn in_nested_session() {
    let _compositor = Compositor::current();
}

/// A launcher this test started; ended when dropped.
pub(crate) struct Launcher {
    /// `None` only while it is dropped.
    component: Option<Component>,
}

impl Launcher {
    /// `deskrinch-launcher` with `args`, once it owns the name.
    pub(crate) fn start(args: &[&str]) -> Self {
        Self::spawn(launcher(args))
    }

    /// `command` (a [`launcher`] command), once it owns the name.
    pub(crate) fn spawn(command: Command) -> Self {
        let launcher = Self {
            component: Some(run(command)),
        };
        assert!(
            eventually(|| owner_pid() == Some(launcher.id())),
            "the launcher did not take its name"
        );
        launcher
    }

    fn component(&self) -> &Component {
        self.component.as_ref().expect("dropped only once")
    }

    /// Its process id.
    pub(crate) fn id(&self) -> u32 {
        self.component().id()
    }

    /// The frames it has presented (rinch's `RINCH_PERF` lines).
    pub(crate) fn frames(&self) -> u64 {
        self.component().watch().frames()
    }

    /// Waits until it has presented, with the run's renderer.
    pub(crate) fn wait_presented(&self) {
        if let Err(why) = self.component().wait_presented_with(Renderer::of_run()) {
            panic!("the launcher: {why}");
        }
    }

    /// Its last lines, for a failure's message.
    pub(crate) fn log(&self) -> String {
        self.component().watch().tail()
    }

    /// Whether a line it wrote has `text` in it.
    pub(crate) fn said(&self, text: &str) -> bool {
        self.component()
            .watch()
            .wait_for_line(Duration::ZERO, |line| line.contains(text))
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        // Killed and reaped; its name goes with it, for the next test.
        drop(self.component.take());
        assert!(eventually(|| owner_pid().is_none()));
    }
}

/// The launcher binary with `args`, for [`run`] or [`Launcher::spawn`].
pub(crate) fn launcher(args: &[&str]) -> Command {
    let mut command = Command::new(LAUNCHER);
    command.args(args);
    command
}

/// Starts `command` (a [`launcher`] command) with the run's renderer,
/// logging its frames; killed when dropped. A launcher it starts in turn
/// inherits its output, which the component's watch reads too.
pub(crate) fn run(command: Command) -> Component {
    Component::start(command, Renderer::of_run()).unwrap()
}

/// Runs `deskrinch-launcher` with `args` to its end.
///
/// Not started with `deskrinch_test_lifetime::spawn`: it ends by itself,
/// asking the running launcher over the bus and leaving, bounded by the
/// bus call's timeout. (With none running, `toggle` and `show` would start
/// one in a process group of its own; the tests call it so only while one
/// runs, and the one test that has it start one,
/// `with_none_running_toggle_starts_one_shown`, runs it as a component and
/// ends what it started itself.)
pub(crate) fn client(args: &[&str]) -> ExitStatus {
    launcher(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
}

/// The process that owns the launcher's name on the session bus.
pub(crate) fn owner_pid() -> Option<u32> {
    zbus::block_on(async {
        let connection = Bus::Session.connect().await.unwrap();
        let reply = connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "GetConnectionUnixProcessID",
                &(NAME,),
            )
            .await
            .ok()?;
        reply.body().deserialize::<u32>().ok()
    })
}

/// Whether `done` comes true within [`PATIENCE`].
pub(crate) fn eventually(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    done()
}

/// Whether `holds` stays true for [`QUIET`].
pub(crate) fn stays(mut holds: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + QUIET;
    while Instant::now() < deadline {
        if !holds() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    holds()
}

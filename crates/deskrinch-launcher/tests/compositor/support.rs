//! What the tests share: the launcher binary run and ended, the bus asked
//! who owns the launcher's name, and waiting.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deskrinch_bus::Bus;
use deskrinch_launcher::NAME;
use deskrinch_test_client::Compositor;

/// How long something that should happen gets.
pub(crate) const PATIENCE: Duration = Duration::from_secs(30);
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

/// What a process writes to standard error, as it comes: its presented
/// frames (rinch's `RINCH_PERF` lines) counted, every other line kept.
pub(crate) struct Stderr {
    frames: AtomicU64,
    lines: Mutex<Vec<String>>,
}

impl Stderr {
    /// Reads `stream` on a thread of its own until it ends.
    pub(crate) fn watch(stream: impl Read + Send + 'static) -> Arc<Self> {
        let stderr = Arc::new(Self {
            frames: AtomicU64::new(0),
            lines: Mutex::new(Vec::new()),
        });
        let reader = Arc::clone(&stderr);
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if line.starts_with("[PERF] frame ") {
                    reader.frames.fetch_add(1, Ordering::SeqCst);
                } else {
                    reader.lines.lock().unwrap().push(line);
                }
            }
        });
        stderr
    }

    /// The frames presented so far.
    pub(crate) fn frames(&self) -> u64 {
        self.frames.load(Ordering::SeqCst)
    }

    /// Every line other than a frame's, for a failure's message.
    pub(crate) fn log(&self) -> String {
        self.lines.lock().unwrap().join("\n")
    }

    /// Whether a line other than a frame's has `text` in it.
    pub(crate) fn said(&self, text: &str) -> bool {
        self.lines
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains(text))
    }
}

/// A launcher this test started; ended when dropped.
pub(crate) struct Launcher {
    pub(crate) child: Child,
    pub(crate) stderr: Arc<Stderr>,
}

impl Launcher {
    /// `deskrinch-launcher` with `args`, once it owns the name.
    pub(crate) fn start(args: &[&str]) -> Self {
        Self::spawn(&mut launcher(args))
    }

    /// `command` (a [`launcher`] command), once it owns the name.
    pub(crate) fn spawn(command: &mut Command) -> Self {
        let mut child = command.spawn().unwrap();
        let stderr = Stderr::watch(child.stderr.take().unwrap());
        let launcher = Self { child, stderr };
        assert!(
            eventually(|| owner_pid() == Some(launcher.child.id())),
            "the launcher did not take its name"
        );
        launcher
    }

    pub(crate) fn frames(&self) -> u64 {
        self.stderr.frames()
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
        // Its name goes with it, for the next test.
        assert!(eventually(|| owner_pid().is_none()));
    }
}

/// The launcher binary with `args`, drawing with the software renderer
/// and logging its frames.
pub(crate) fn launcher(args: &[&str]) -> Command {
    let mut command = Command::new(LAUNCHER);
    command
        .args(args)
        .env("RINCH_PERF", "1")
        .env("RINCH_RENDERER", "software")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

/// Runs `deskrinch-launcher` with `args` to its end.
pub(crate) fn client(args: &[&str]) -> ExitStatus {
    let mut command = Command::new(LAUNCHER);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.status().unwrap()
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

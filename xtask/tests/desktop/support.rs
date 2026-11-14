//! What the desktop tests share: starting `cargo xtask desktop` and reading
//! what it says, talking to its bus, the outer session's windows, the keys
//! of the shell's chords, and profiles of the tests' own.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_shell::{Key as ShellKey, Modifier};
use deskrinch_test_client::Compositor;
use deskrinch_test_input::Key;
use deskrinch_test_wait::{Deadline, PATIENCE, POLL, eventually};
use deskrinch_window_management::WindowManagement;
use rustix::process::{Pid, Signal, kill_process};

/// How long a desktop has to be ready: the first builds its components.
pub(crate) const READY: Duration = Duration::from_mins(15);
/// The launcher's name, object and interface on the desktop's bus.
pub(crate) const LAUNCHER: &str = "org.deskrinch.Launcher";
pub(crate) const LAUNCHER_PATH: &str = "/org/deskrinch/Launcher";
pub(crate) const LAUNCHER_INTERFACE: &str = "org.deskrinch.Launcher1";

/// A running `cargo xtask desktop`, as its lines on standard output
/// describe it once ready.
pub(crate) struct Desktop {
    child: Child,
    /// The nested session's root.
    pub(crate) root: PathBuf,
    /// The nested session's bus.
    pub(crate) bus: String,
    /// Where its standard error goes, for a failure's message.
    stderr: PathBuf,
    _dir: tempfile::TempDir,
}

/// `cargo xtask desktop` under the session's compositor, its
/// components unoptimised and drawing in software, with `options`, and
/// `command` after `--` when there is one.
pub(crate) fn desktop_command(options: &[&str], command: &[&str], stderr: &Path) -> Command {
    let compositor = Compositor::current();
    let mut xtask = Command::new(env!("CARGO_BIN_EXE_xtask"));
    xtask
        .args(["desktop", compositor_name(compositor)])
        .args(["--debug", "--renderer", "software"])
        .args(options);
    if !command.is_empty() {
        xtask.arg("--").args(command);
    }
    xtask
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(File::create(stderr).unwrap());
    xtask
}

pub(crate) fn compositor_name(compositor: Compositor) -> &'static str {
    match compositor {
        Compositor::Kwin => "kwin",
        Compositor::Sway => "sway",
        Compositor::Labwc => "labwc",
    }
}

/// Starts a desktop and waits until it says it is ready.
pub(crate) fn start(options: &[&str], command: &[&str]) -> Desktop {
    let dir = tempfile::tempdir().unwrap();
    let stderr = dir.path().join("stderr");
    let mut child = desktop_command(options, command, &stderr).spawn().unwrap();
    let lines = lines(child.stdout.take().unwrap());
    let (mut root, mut bus) = (None, None);
    let deadline = Instant::now() + READY;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let Ok(line) = lines.recv_timeout(left) else {
            let _killed = child.kill();
            let status = child.wait().unwrap();
            panic!(
                "the desktop never said it was ready ({status}); it said:\n{}",
                fs::read_to_string(&stderr).unwrap_or_default()
            );
        };
        if let Some(path) = line.strip_prefix("desktop: session ") {
            root = Some(PathBuf::from(path));
        } else if let Some(address) = line.strip_prefix("desktop: bus ") {
            bus = Some(address.to_owned());
        } else if line.starts_with("desktop: ready") {
            break;
        }
    }
    Desktop {
        child,
        root: root.expect("the desktop named its session"),
        bus: bus.expect("the desktop named its bus"),
        stderr,
        _dir: dir,
    }
}

/// The lines `stdout` gives, as they come.
pub(crate) fn lines(stdout: impl std::io::Read + Send + 'static) -> Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            // Read on once no one listens, so that the desktop and what it
            // runs never write into a pipe that is not drained.
            let _sent = sender.send(line);
        }
    });
    receiver
}

impl Desktop {
    /// Waits for it to end; how it did.
    pub(crate) fn wait(&mut self) -> ExitStatus {
        self.wait_within(PATIENCE)
    }

    /// Waits up to `bound` for it to end; how it did. For a desktop whose
    /// command is itself a run of waits of the patience, which says so.
    pub(crate) fn wait_within(&mut self, bound: Duration) -> ExitStatus {
        let deadline = Deadline::after(bound);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                !deadline.passed(),
                "waited {bound:?} for the desktop to end; it said:\n{}",
                self.said()
            );
            thread::sleep(POLL);
        }
    }

    /// Sends it `signal`.
    pub(crate) fn signal(&self, signal: Signal) {
        let pid = Pid::from_raw(i32::try_from(self.child.id()).unwrap()).unwrap();
        kill_process(pid, signal).unwrap();
    }

    /// What it wrote on standard error.
    pub(crate) fn said(&self) -> String {
        fs::read_to_string(&self.stderr).unwrap_or_default()
    }

    /// Waits until its session's root is gone: everything of the
    /// session has ended (the session removes it last).
    pub(crate) fn wait_for_root_gone(&self) {
        eventually("the nested session's directory to go", || {
            !self.root.exists()
        });
    }

    /// What a failure needs said: the outer session's windows as
    /// `windows` lists them, how the nested compositor composites (under
    /// KWin, from its support information), and what the desktop and
    /// its compositor wrote.
    pub(crate) fn state(&self, windows: &impl WindowManagement) -> String {
        let listed: Vec<String> = windows
            .windows()
            .iter()
            .map(|window| format!("{:?} {:?}", window.title, window.state))
            .collect();
        format!(
            "the outer windows: {listed:?}; the nested compositing: {}\n\
             the desktop said:\n{}\nits compositor said:\n{}",
            self.compositing(),
            self.said(),
            fs::read_to_string(self.root.join("compositor.log")).unwrap_or_default()
        )
    }

    /// The `Compositing Type:` line of the nested KWin's support
    /// information, or why there is none.
    pub(crate) fn compositing(&self) -> String {
        if Compositor::current() != Compositor::Kwin {
            return "(not KWin)".into();
        }
        let info: zbus::Result<String> = zbus::block_on(async {
            let reply = self
                .connect()
                .await
                .call_method(
                    Some("org.kde.KWin"),
                    "/KWin",
                    Some("org.kde.KWin"),
                    "supportInformation",
                    &(),
                )
                .await?;
            reply.body().deserialize()
        });
        match info {
            Ok(info) => info
                .lines()
                .find(|line| line.trim().starts_with("Compositing Type:"))
                .unwrap_or("(no compositing type)")
                .trim()
                .to_owned(),
            Err(error) => format!("(KWin did not say: {error})"),
        }
    }

    /// A connection to the desktop's bus.
    pub(crate) async fn connect(&self) -> zbus::Connection {
        zbus::connection::Builder::address(self.bus.as_str())
            .build()
            .await
            .unwrap()
    }

    /// Toggles the launcher through the desktop's bus; whether it is now
    /// shown.
    pub(crate) fn toggle_launcher(&self) -> bool {
        zbus::block_on(async {
            let connection = self.connect().await;
            let reply = connection
                .call_method(
                    Some(LAUNCHER),
                    LAUNCHER_PATH,
                    Some(LAUNCHER_INTERFACE),
                    "Toggle",
                    &(),
                )
                .await
                .unwrap();
            reply.body().deserialize::<bool>().unwrap()
        })
    }

    /// The launcher's `Shown` property.
    pub(crate) fn launcher_shown(&self) -> bool {
        zbus::block_on(async {
            let connection = self.connect().await;
            let reply = connection
                .call_method(
                    Some(LAUNCHER),
                    LAUNCHER_PATH,
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &(LAUNCHER_INTERFACE, "Shown"),
                )
                .await
                .unwrap();
            let value: zbus::OwnedVariant = reply.body().deserialize().unwrap();
            bool::try_from(value).unwrap()
        })
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        // A test that failed early still ends what it started.
        if matches!(self.child.try_wait(), Ok(None)) {
            self.signal(Signal::TERM);
            let _ended = self.child.wait();
        }
    }
}

/// The outer session's windows, as its window management lists them.
pub(crate) fn outer_windows() -> deskrinch_window_management::WindowManager {
    let manager = deskrinch_window_management::connect(|_| {}).unwrap();
    eventually("the desktop's window", || !manager.windows().is_empty());
    manager
}

/// The test-input key for a modifier of the shell's chords.
pub(crate) fn modifier_key(modifier: Modifier) -> Key {
    match modifier {
        Modifier::Control => Key::LEFT_CTRL,
        Modifier::Alt => Key::LEFT_ALT,
        Modifier::Shift => Key::LEFT_SHIFT,
        Modifier::Super => Key::LEFT_META,
    }
}

/// The test-input key for a key of the shell's chords.
pub(crate) fn key(key: ShellKey) -> Key {
    match key {
        ShellKey::Space => Key::SPACE,
    }
}

/// The environment a command printed with `env`.
pub(crate) fn environment(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

/// A profile of the test's own in the workspace, removed when dropped.
pub(crate) struct TestProfile {
    pub(crate) name: String,
    pub(crate) dir: PathBuf,
}

impl TestProfile {
    pub(crate) fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let name = format!("test-{}-{nanos}", std::process::id());
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.work/desktop")
            .join(&name);
        Self { name, dir }
    }
}

impl Drop for TestProfile {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.dir);
    }
}

/// Every file under `dir`, by its path relative to `dir`, with its
/// contents; the lock and the claim aside.
pub(crate) fn files(dir: &Path) -> HashMap<PathBuf, Vec<u8>> {
    let mut found = HashMap::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(dir).unwrap().to_owned();
            if relative == Path::new(".lock") || relative == Path::new(".session") {
                continue;
            }
            let kind = fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                found.insert(relative, fs::read(&path).unwrap());
            }
        }
    }
    found
}

//! `cargo xtask desktop` end to end, nested: each test runs it inside the
//! headless session `cargo xtask compositor-tests` runs this binary in, so
//! that its window opens there, never on the real desktop. The nested
//! compositor is the session's own kind (`DESKRINCH_TEST_COMPOSITOR`).
//!
//! Ignored by a plain `cargo test`; `cargo xtask compositor-tests` runs
//! them, granting this binary under KWin the window management that closes
//! the desktop's window and the fake input that types its chord.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod desktop {
    use std::collections::HashMap;
    use std::fs::{self, File};
    use std::io::{BufRead, BufReader};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::sync::mpsc::{self, Receiver};
    use std::thread;
    use std::time::{Duration, Instant};

    use deskrinch_shell::{Key as ShellKey, LAUNCHER_CHORD, Modifier};
    use deskrinch_test_client::Compositor;
    use deskrinch_test_input::{Input, Key};
    use deskrinch_window_management::{Action, WindowManagement};
    use rustix::process::{Pid, Signal, kill_process};

    /// How long a desktop has to be ready: the first builds its components.
    const READY: Duration = Duration::from_mins(15);
    /// How long anything else is waited for.
    const PATIENCE: Duration = Duration::from_secs(60);
    /// The launcher's name, object and interface on the desktop's bus.
    const LAUNCHER: &str = "org.deskrinch.Launcher";
    const LAUNCHER_PATH: &str = "/org/deskrinch/Launcher";
    const LAUNCHER_INTERFACE: &str = "org.deskrinch.Launcher1";

    /// A running `cargo xtask desktop`, as its lines on standard output
    /// describe it once ready.
    struct Desktop {
        child: Child,
        /// The nested session's root.
        root: PathBuf,
        /// The nested session's bus.
        bus: String,
        /// Where its standard error goes, for a failure's message.
        stderr: PathBuf,
        _dir: tempfile::TempDir,
    }

    /// `cargo xtask desktop` under the session's compositor, its
    /// components unoptimised and drawing in software, with `options`, and
    /// `command` after `--` when there is one.
    fn desktop_command(options: &[&str], command: &[&str], stderr: &Path) -> Command {
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

    fn compositor_name(compositor: Compositor) -> &'static str {
        match compositor {
            Compositor::Kwin => "kwin",
            Compositor::Sway => "sway",
            Compositor::Labwc => "labwc",
        }
    }

    /// Starts a desktop and waits until it says it is ready.
    fn start(options: &[&str], command: &[&str]) -> Desktop {
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
    fn lines(stdout: impl std::io::Read + Send + 'static) -> Receiver<String> {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        receiver
    }

    impl Desktop {
        /// Waits for it to end; how it did.
        fn wait(&mut self) -> ExitStatus {
            let deadline = Instant::now() + PATIENCE;
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    return status;
                }
                assert!(
                    Instant::now() < deadline,
                    "the desktop did not end; it said:\n{}",
                    self.said()
                );
                thread::sleep(Duration::from_millis(50));
            }
        }

        /// Sends it `signal`.
        fn signal(&self, signal: Signal) {
            let pid = Pid::from_raw(i32::try_from(self.child.id()).unwrap()).unwrap();
            kill_process(pid, signal).unwrap();
        }

        /// What it wrote on standard error.
        fn said(&self) -> String {
            fs::read_to_string(&self.stderr).unwrap_or_default()
        }

        /// Waits until its session's root is gone: everything of the
        /// session has ended (the session removes it last).
        fn wait_for_root_gone(&self) {
            until(
                || !self.root.exists(),
                "the nested session's directory to go",
            );
        }

        /// A connection to the desktop's bus.
        async fn connect(&self) -> zbus::Connection {
            zbus::connection::Builder::address(self.bus.as_str())
                .build()
                .await
                .unwrap()
        }

        /// Toggles the launcher through the desktop's bus; whether it is now
        /// shown.
        fn toggle_launcher(&self) -> bool {
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
        fn launcher_shown(&self) -> bool {
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

    /// Waits until `condition` holds, for at most [`PATIENCE`].
    fn until(condition: impl Fn() -> bool, what: &str) {
        let deadline = Instant::now() + PATIENCE;
        while !condition() {
            assert!(Instant::now() < deadline, "waited in vain for {what}");
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// The outer session's windows, as its window management lists them.
    fn outer_windows() -> deskrinch_window_management::WindowManager {
        let manager = deskrinch_window_management::connect(|_| {}).unwrap();
        until(|| !manager.windows().is_empty(), "the desktop's window");
        manager
    }

    /// The test-input key for a modifier of the shell's chords.
    fn modifier_key(modifier: Modifier) -> Key {
        match modifier {
            Modifier::Control => Key::LEFT_CTRL,
            Modifier::Alt => Key::LEFT_ALT,
            Modifier::Shift => Key::LEFT_SHIFT,
            Modifier::Super => Key::LEFT_META,
        }
    }

    /// The test-input key for a key of the shell's chords.
    fn key(key: ShellKey) -> Key {
        match key {
            ShellKey::Space => Key::SPACE,
        }
    }

    /// The environment a command printed with `env`.
    fn environment(text: &str) -> HashMap<String, String> {
        text.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn starts_the_shell_and_ends_with_its_window() {
        let mut desktop = start(&[], &[]);
        // The launcher runs, hidden, and answers on the desktop's bus.
        assert!(!desktop.launcher_shown());
        assert!(desktop.toggle_launcher());
        assert!(desktop.launcher_shown());
        assert!(!desktop.toggle_launcher());

        let windows = outer_windows();
        for window in windows.windows() {
            windows.perform(window.id, Action::Close).unwrap();
        }
        let status = desktop.wait();
        assert!(status.success(), "{status}; it said:\n{}", desktop.said());
        desktop.wait_for_root_gone();
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_chord_typed_into_its_window_summons_the_launcher() {
        // The keyboard first: on a headless wlroots seat it is the seat's only
        // one, and a window is given the focus as it maps only when there is
        // one.
        let mut input = Input::connect().unwrap();
        let mut desktop = start(&[], &[]);
        let windows = outer_windows();
        let window = windows.windows()[0].id;
        windows.perform(window, Action::Activate).unwrap();
        let modifiers: Vec<Key> = LAUNCHER_CHORD
            .modifiers
            .iter()
            .copied()
            .map(modifier_key)
            .collect();
        input
            .press_with(&modifiers, key(LAUNCHER_CHORD.key))
            .unwrap();
        let deadline = Instant::now() + PATIENCE;
        while !desktop.launcher_shown() {
            assert!(
                Instant::now() < deadline,
                "the chord did not show the launcher; the desktop said:\n{}\n\
                 its compositor said:\n{}",
                desktop.said(),
                fs::read_to_string(desktop.root.join("compositor.log")).unwrap_or_default()
            );
            thread::sleep(Duration::from_millis(100));
        }

        desktop.signal(Signal::TERM);
        let status = desktop.wait();
        assert_eq!(status.code(), Some(130), "it said:\n{}", desktop.said());
        desktop.wait_for_root_gone();
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn a_command_inside_sees_the_nested_session_only() {
        let dir = tempfile::tempdir().unwrap();
        let env_file = dir.path().join("env");
        let script = format!("env > '{}'; exit 3", env_file.display());
        let mut desktop = start(&[], &["sh", "-c", &script]);
        let status = desktop.wait();
        assert_eq!(status.code(), Some(3), "it said:\n{}", desktop.said());
        desktop.wait_for_root_gone();

        let env = environment(&fs::read_to_string(&env_file).unwrap());
        let root = desktop.root.display().to_string();
        assert_eq!(env["HOME"], format!("{root}/home"));
        assert_eq!(env["XDG_RUNTIME_DIR"], format!("{root}/run"));
        assert_eq!(env["DBUS_SESSION_BUS_ADDRESS"], desktop.bus);
        // Not the display or the bus of the session the desktop's window is
        // on: only the nested compositor is given that.
        let outer_bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
        let outer_runtime = std::env::var("XDG_RUNTIME_DIR").unwrap();
        let outer_display = std::env::var("WAYLAND_DISPLAY").unwrap();
        let outer_socket = format!("{outer_runtime}/{outer_display}");
        for (name, value) in &env {
            assert!(
                !value.contains(&outer_bus) && !value.contains(&outer_socket),
                "{name}={value} names the outer session"
            );
        }
        assert!(
            !env["WAYLAND_DISPLAY"].contains('/'),
            "{}",
            env["WAYLAND_DISPLAY"]
        );
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn a_killed_desktop_is_ended_by_its_supervisor() {
        let mut desktop = start(&[], &[]);
        desktop.signal(Signal::KILL);
        let status = desktop.wait();
        assert_eq!(status.code(), None, "killed");
        desktop.wait_for_root_gone();
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn refuses_outside_a_terminal_and_a_session() {
        let dir = tempfile::tempdir().unwrap();
        let stderr = dir.path().join("stderr");
        let mut command = desktop_command(&[], &[], &stderr);
        // No session, no display, no bus: a script's environment.
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .stdout(Stdio::null());
        let status = command.status().unwrap();
        let said = fs::read_to_string(&stderr).unwrap();
        assert_eq!(status.code(), Some(77), "it said:\n{said}");
        assert!(said.contains("only its owner runs it"), "{said}");
        assert!(!said.contains("building"), "it built nothing: {said}");
    }

    /// A profile of the test's own in the workspace, removed when dropped.
    struct TestProfile {
        name: String,
        dir: PathBuf,
    }

    impl TestProfile {
        fn new() -> Self {
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

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn a_profile_keeps_its_home_for_one_desktop_at_a_time() {
        let profile = TestProfile::new();
        let options = ["--profile", profile.name.as_str()];
        let mut first = start(&options, &["sh", "-c", "echo kept > \"$HOME/mark\""]);
        assert!(first.wait().success(), "it said:\n{}", first.said());
        first.wait_for_root_gone();
        assert_eq!(
            fs::read_to_string(profile.dir.join("home/mark")).unwrap(),
            "kept\n"
        );

        let dir = tempfile::tempdir().unwrap();
        let seen = dir.path().join("seen");
        let script = format!("cat \"$HOME/mark\" > '{}'; sleep 600", seen.display());
        let mut second = start(&options, &["sh", "-c", &script]);
        until(
            || fs::read_to_string(&seen).is_ok_and(|text| text.ends_with('\n')),
            "the second desktop to read its home",
        );
        assert_eq!(fs::read_to_string(&seen).unwrap(), "kept\n");

        let stderr = dir.path().join("third");
        let third = desktop_command(&options, &[], &stderr)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        let said = fs::read_to_string(&stderr).unwrap();
        assert_eq!(third.code(), Some(70), "it said:\n{said}");
        assert!(said.contains("is in use"), "{said}");

        second.signal(Signal::TERM);
        assert_eq!(
            second.wait().code(),
            Some(130),
            "it said:\n{}",
            second.said()
        );
        second.wait_for_root_gone();
        assert!(profile.dir.join("home/mark").exists(), "the home is kept");
    }
}

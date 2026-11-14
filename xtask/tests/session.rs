//! `cargo xtask session` end to end: the session it gives a command, the
//! status it passes on, and that nothing of the session outlives it.
//!
//! Ignored by a plain `cargo test`, because they need a compositor
//! installed; they start only isolated sessions of their own, so they are
//! safe to run anywhere. Name the compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test session -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod session {
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use std::process::{Child, Command, Output, Stdio};

    use deskrinch_test_wait::wait_for;
    use rustix::process::{Pid, Signal, kill_process, test_kill_process};

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// `xtask session <compositor> <command…>`, with a real-looking session
    /// in its own environment for the session to remove.
    fn session(command: &[&str]) -> Command {
        session_with(&[], command)
    }

    /// [`session`] with options for `xtask session` before the compositor.
    fn session_with(options: &[&str], command: &[&str]) -> Command {
        let mut xtask = Command::new(env!("CARGO_BIN_EXE_xtask"));
        xtask
            .arg("session")
            .args(options)
            .arg(compositor())
            .args(command)
            .envs(REAL_LOOKING)
            .stdin(Stdio::null());
        xtask
    }

    /// A real session's variables, as an agent's shell would have them.
    const REAL_LOOKING: [(&str, &str); 13] = [
        ("DISPLAY", ":99"),
        ("XAUTHORITY", "/nonexistent/xauthority"),
        ("WAYLAND_DISPLAY", "wayland-real"),
        ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus"),
        ("XDG_CURRENT_DESKTOP", "KDE"),
        (
            "XDG_CONFIG_DIRS",
            "/nonexistent/home/.config/kdedefaults:/etc/xdg",
        ),
        (
            "XDG_DATA_DIRS",
            "/nonexistent/flatpak/exports/share:/usr/share",
        ),
        ("XDG_SESSION_ID", "1"),
        ("XDG_SESSION_CLASS", "user"),
        ("XDG_SESSION_DESKTOP", "KDE"),
        ("XDG_SESSION_TYPE", "wayland"),
        ("XDG_SEAT", "seat0"),
        ("XDG_VTNR", "1"),
    ];

    /// The environment a command printed with `env`.
    fn environment(stdout: &[u8]) -> HashMap<String, String> {
        String::from_utf8_lossy(stdout)
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
    }

    fn run(command: &[&str]) -> Output {
        session(command).output().unwrap()
    }

    /// Whether the process `pid` (as text, from a pid file) still runs.
    fn alive(pid: &str) -> bool {
        let pid = Pid::from_raw(pid.trim().parse().unwrap()).unwrap();
        test_kill_process(pid).is_ok()
    }

    fn wait_for_file(path: &Path) -> String {
        wait_for(
            &format!("{} to be written", path.display()),
            || match fs::read_to_string(path) {
                Ok(text) if text.ends_with('\n') => Ok(text),
                other => Err(other.map_err(|error| error.kind())),
            },
        )
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn passes_the_commands_status_on() {
        assert_eq!(run(&["sh", "-c", "exit 3"]).status.code(), Some(3));
        assert_eq!(run(&["true"]).status.code(), Some(0));
        let killed = run(&["sh", "-c", "kill -TERM $$"]);
        assert_eq!(killed.status.code(), Some(128 + 15));
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn gives_the_command_the_session_and_not_the_real_one() {
        let here = tempfile::tempdir().unwrap();
        let output = session(&["sh", "-c", "env; echo PWD=$(pwd)"])
            .current_dir(here.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let env = environment(&output.stdout);
        let env: HashMap<&str, &str> = env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        // The tests' guard against the real session reads it.
        assert_eq!(
            env.get("DESKRINCH_TEST_COMPOSITOR").copied(),
            Some(compositor().as_str())
        );

        for real in [
            "DISPLAY",
            "XAUTHORITY",
            "WAYLAND_SOCKET",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_ID",
            "XDG_SESSION_CLASS",
            "XDG_SESSION_DESKTOP",
            "XDG_SESSION_TYPE",
            "XDG_SEAT",
            "XDG_VTNR",
        ] {
            assert!(!env.contains_key(real), "{real} leaked: {:?}", env[real]);
        }
        assert_eq!(env["XDG_CONFIG_DIRS"], "/etc/xdg");
        assert_eq!(env["XDG_DATA_DIRS"], "/usr/local/share/:/usr/share/");
        let root = env["DESKRINCH_SESSION_DIR"];
        assert!(root.starts_with("/tmp/deskrinch-session-"), "{root}");
        // cargo and rustup keep the real homes while HOME moves.
        let real = |variable: &str, default: &str| {
            std::env::var(variable)
                .unwrap_or_else(|_| format!("{}/{default}", std::env::var("HOME").unwrap()))
        };
        assert_eq!(env["CARGO_HOME"], real("CARGO_HOME", ".cargo"));
        assert_eq!(env["RUSTUP_HOME"], real("RUSTUP_HOME", ".rustup"));
        for (variable, sub) in [
            ("HOME", "home"),
            ("XDG_RUNTIME_DIR", "run"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_STATE_HOME", "state"),
        ] {
            assert_eq!(env[variable], format!("{root}/{sub}"), "{variable}");
        }
        let socket = env["WAYLAND_DISPLAY"];
        assert!(socket.starts_with("wayland-"), "{socket}");
        assert_ne!(socket, "wayland-real");
        assert!(
            env["DBUS_SESSION_BUS_ADDRESS"].starts_with("unix:abstract=/tmp/dbus-deskrinch-"),
            "{}",
            env["DBUS_SESSION_BUS_ADDRESS"]
        );
        if compositor() == "sway" {
            assert!(env["SWAYSOCK"].starts_with(&format!("{root}/run/")));
        }
        // The command runs where it was asked from.
        assert_eq!(
            Path::new(env["PWD"]).canonicalize().unwrap(),
            here.path().canonicalize().unwrap()
        );
        // And the session is gone afterwards.
        assert!(!Path::new(root).exists(), "{root} was left behind");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn sets_the_current_desktop_only_when_asked() {
        let output = session_with(&["--current-desktop", "deskrinch"], &["env"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let env = environment(&output.stdout);
        assert_eq!(env["XDG_CURRENT_DESKTOP"], "deskrinch");
    }

    /// The session checks every output before the command starts: the
    /// whole scale `wl_output` reports, and the logical size `xdg-output`
    /// reports against the mode (853x480 for 1280x720 at 1.5, which a
    /// compositor that rounded up to 2 would report as 640x360), and fails
    /// (70) if either is off; so a command that runs at all runs at the scale
    /// asked for.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn starts_the_command_at_the_scale_asked_for() {
        for scale in ["2", "1.5"] {
            let output = session_with(&["--outputs", "2", "--scale", scale], &["true"])
                .output()
                .unwrap();
            assert!(output.status.success(), "scale {scale}: {output:?}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains(&format!("at scale {scale}")), "{stderr}");
        }
    }

    /// As for the scale: the session checks each output's transform, and
    /// its logical size turned with it, before the command starts.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn starts_the_command_with_the_transform_asked_for() {
        for (options, said) in [
            (&["--transform", "90"][..], "transform 90"),
            (
                &["--scale", "2", "--transform", "270"][..],
                "at scale 2, transform 270",
            ),
        ] {
            let output = session_with(options, &["true"]).output().unwrap();
            assert!(output.status.success(), "{options:?}: {output:?}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains(said), "{stderr}");
        }
    }

    /// `xtask session-supervisor <root>`, its standard input closed at once
    /// (as when xtask has died).
    fn supervisor(root: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("session-supervisor")
            .arg(root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    #[test]
    fn the_supervisor_refuses_a_directory_that_is_not_a_session_s() {
        // Named as a session's and where a session's is, but not marked.
        let unmarked = tempfile::Builder::new()
            .prefix("deskrinch-session-")
            .tempdir_in("/tmp")
            .unwrap();
        fs::write(unmarked.path().join("keep"), "").unwrap();
        let output = supervisor(unmarked.path());
        assert!(!output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty(), "it said it was ready: {output:?}");
        assert!(unmarked.path().join("keep").is_file());
    }

    #[test]
    fn the_supervisor_refuses_a_marked_directory_outside_the_temporary_one() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("deskrinch-session-x");
        fs::create_dir(&root).unwrap();
        fs::write(root.join(".deskrinch-session"), "").unwrap();
        let output = supervisor(&root);
        assert!(!output.status.success(), "{output:?}");
        assert!(root.join(".deskrinch-session").is_file());
    }

    #[test]
    fn refuses_a_scale_that_is_not_positive() {
        for scale in ["0", "-1", "inf", "two"] {
            let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
                .args(["session", &format!("--scale={scale}"), "sway", "true"])
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(!output.status.success(), "scale {scale}: {output:?}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("--scale"), "{stderr}");
        }
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn ends_what_the_command_left_running() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!("sleep 600 & echo $! > {}", pid_file.display());
        let output = run(&["sh", "-c", &script]);
        assert!(output.status.success(), "{output:?}");
        assert!(!alive(&fs::read_to_string(&pid_file).unwrap()));
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn kills_and_reports_a_process_that_left_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!(
            "setsid sleep 600 & echo $! > {}; sleep 0.5",
            pid_file.display()
        );
        let output = run(&["sh", "-c", &script]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(70), "{stderr}");
        assert!(stderr.contains("outlived it"), "{stderr}");
        assert!(!alive(&fs::read_to_string(&pid_file).unwrap()));
        // The session's directory is kept for a look; this test removes it.
        let kept = stderr
            .lines()
            .find_map(|line| line.strip_prefix("session: kept "))
            .expect("the kept directory");
        assert!(kept.starts_with("/tmp/deskrinch-session-"), "{kept}");
        fs::remove_dir_all(kept).unwrap();
    }

    /// The processes whose parent is `pid`, with their command lines.
    fn children_of(pid: u32) -> Vec<(u32, String)> {
        fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter_map(|entry| {
                let child: u32 = entry.file_name().to_str()?.parse().ok()?;
                let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
                // The parent is the second field after the command name's
                // closing parenthesis.
                let parent: u32 = stat
                    .rsplit_once(')')?
                    .1
                    .split_whitespace()
                    .nth(1)?
                    .parse()
                    .ok()?;
                let line = fs::read(entry.path().join("cmdline")).ok()?;
                (parent == pid).then(|| (child, String::from_utf8_lossy(&line).replace('\0', " ")))
            })
            .collect()
    }

    /// xtask killed (`SIGKILL`: no handler runs) takes its session with it:
    /// the session's supervisor ends the compositor (KWin's binary has file
    /// capabilities, so the kernel's parent-death signal cannot), the
    /// command, and the directory; the bus's daemon dies with xtask.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_killed_xtask_takes_its_session_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let report = dir.path().join("report");
        let script = format!(
            "echo $$ $DESKRINCH_SESSION_DIR > {}; exec sleep 600",
            report.display()
        );
        let mut command = session(&["sh", "-c", &script]);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        // Bound to this test, should it be killed itself.
        let mut xtask = deskrinch_test_lifetime::spawn(command).unwrap();
        let line = wait_for_file(&report);
        let (pid, root) = line.trim().split_once(' ').unwrap();
        let (pid, root): (u32, &Path) = (pid.parse().unwrap(), Path::new(root));
        assert!(root.is_dir());
        // The session's processes that are xtask's children: the
        // compositor and the bus's daemon (not the supervisor).
        let session: Vec<(u32, String)> = children_of(xtask.id())
            .into_iter()
            .filter(|(_, line)| !line.contains("session-supervisor"))
            .collect();
        let has = |what: &str| session.iter().any(|(_, line)| line.contains(what));
        assert!(has(&compositor()), "no compositor in {session:?}");
        assert!(has("dbus-daemon"), "no bus in {session:?}");

        xtask.kill().unwrap();
        xtask.wait().unwrap();

        // Gone within the patience: the supervisor gives the group five
        // seconds to end on `SIGTERM` before `SIGKILL`, the floor of how
        // long it can take.
        let left = || {
            let mut left: Vec<String> = session
                .iter()
                .filter(|(pid, _)| deskrinch_test_lifetime::running(*pid))
                .map(|(_, line)| line.clone())
                .collect();
            if deskrinch_test_lifetime::running(pid) {
                left.push("the command".into());
            }
            if root.exists() {
                left.push(root.display().to_string());
            }
            left
        };
        wait_for("the session to go", || {
            let left = left();
            if left.is_empty() { Ok(()) } else { Err(left) }
        });
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn an_interrupt_ends_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!("echo $$ > {}; exec sleep 600", pid_file.display());
        let mut xtask: Child = session(&["sh", "-c", &script])
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = wait_for_file(&pid_file);
        kill_process(Pid::from_child(&xtask), Signal::INT).unwrap();
        let status = xtask.wait().unwrap();
        assert_eq!(status.code(), Some(130));
        assert!(!alive(&pid));
    }
}

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
    use std::time::{Duration, Instant};

    use rustix::process::{Pid, Signal, kill_process, test_kill_process};

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// `xtask session <compositor> <command…>`, with a real-looking session
    /// in its own environment for the session to remove.
    fn session(command: &[&str]) -> Command {
        let mut xtask = Command::new(env!("CARGO_BIN_EXE_xtask"));
        xtask
            .arg("session")
            .arg(compositor())
            .args(command)
            .env("DISPLAY", ":99")
            .env("XAUTHORITY", "/nonexistent/xauthority")
            .env("WAYLAND_DISPLAY", "wayland-real")
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")
            .stdin(Stdio::null());
        xtask
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
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(text) = fs::read_to_string(path)
                && text.ends_with('\n')
            {
                return text;
            }
            assert!(
                Instant::now() < deadline,
                "{} never appeared",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
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
        let text = String::from_utf8(output.stdout).unwrap();
        let env: HashMap<_, _> = text
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect();

        for real in ["DISPLAY", "XAUTHORITY", "WAYLAND_SOCKET"] {
            assert!(!env.contains_key(real), "{real} leaked: {:?}", env[real]);
        }
        let root = env["DESKRINCH_SESSION_DIR"];
        assert!(root.starts_with("/tmp/deskrinch-session-"), "{root}");
        for (variable, sub) in [
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

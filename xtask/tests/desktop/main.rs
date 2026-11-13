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
mod support;

#[cfg(test)]
mod desktop {
    use std::fs::{self, File};
    use std::path::Path;
    use std::process::Stdio;
    use std::thread;
    use std::time::{Duration, Instant};

    use deskrinch_shell::LAUNCHER_CHORD;
    use deskrinch_test_input::{Input, Key};
    use deskrinch_window_management::{Action, WindowManagement, WindowState};
    use rustix::process::Signal;

    use super::support::{
        PATIENCE, TestProfile, desktop_command, environment, files, key, modifier_key,
        outer_windows, start, start_logging, until,
    };

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
    fn the_panel_follows_the_windows_in_the_desktop() {
        // Under KWin only what the shell grants the panel lets it see the
        // window list; sway and labwc restrict none.
        let mut desktop = start_logging("deskrinch_panel=info");
        until(
            || desktop.said().contains("following the windows through"),
            "the panel to follow the windows",
        );
        let said = desktop.said();
        assert!(!said.contains("the taskbar shows none"), "{said}");
        desktop.signal(Signal::TERM);
        let status = desktop.wait();
        assert_eq!(status.code(), Some(130), "it said:\n{}", desktop.said());
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
        // The chord goes to the window with the keyboard focus: typed before
        // the outer compositor says the desktop's window has it, it would go
        // nowhere.
        let deadline = Instant::now() + PATIENCE;
        while !windows
            .windows()
            .iter()
            .any(|shown| shown.id == window && shown.state.contains(WindowState::ACTIVE))
        {
            assert!(
                Instant::now() < deadline,
                "the desktop's window never had the keyboard focus: {}",
                desktop.state(&windows)
            );
            thread::sleep(Duration::from_millis(100));
        }
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
                "the chord did not show the launcher: {}",
                desktop.state(&windows)
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

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn a_killed_desktop_leaves_its_profile_whole_and_free() {
        let profile = TestProfile::new();
        let options = ["--profile", profile.name.as_str()];
        let dir = tempfile::tempdir().unwrap();
        let written = dir.path().join("written");
        let script = format!(
            "echo kept > \"$HOME/mark\" && echo kept > \"$XDG_CONFIG_HOME/mark\" && \
             touch '{}'; sleep 600",
            written.display()
        );
        let mut killed = start(&options, &["sh", "-c", &script]);
        until(|| written.exists(), "the desktop to write its profile");
        let before = files(&profile.dir);
        killed.signal(Signal::KILL);
        assert_eq!(killed.wait().code(), None, "killed");
        killed.wait_for_root_gone();

        // The supervisor removed the session's links, not what they lead to:
        // every file is still there (a compositor may still rewrite its own
        // configuration as it ends), and what the command wrote is intact.
        let after = files(&profile.dir);
        for path in before.keys() {
            assert!(after.contains_key(path), "{} is gone", path.display());
        }
        assert_eq!(after[Path::new("home/mark")], b"kept\n");
        assert_eq!(after[Path::new("config/mark")], b"kept\n");
        // No lock is left held.
        let lock = File::open(profile.dir.join(".lock")).unwrap();
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .expect("the lock is free");
        drop(lock);

        // The claim no longer stands in the way: a new desktop starts on it.
        let mut next = start(&options, &[]);
        assert!(!next.launcher_shown(), "its launcher runs, hidden");
        next.signal(Signal::TERM);
        assert_eq!(next.wait().code(), Some(130), "it said:\n{}", next.said());
        next.wait_for_root_gone();
        assert!(
            !profile.dir.join(".session").exists(),
            "its claim ended with it"
        );
    }
}

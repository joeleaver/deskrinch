//! `cargo xtask measure` end to end, on stand-ins for a rinch program: shell
//! scripts that print rinch's `[PERF] frame N:` lines (or do not), in the
//! isolated sessions the task starts. What is checked is the orchestration:
//! the report's sections, the errors for a program that exits or never draws,
//! and that each session is ended afterwards. Nothing here asserts on time.
//!
//! The Callgrind path (`--instructions`) is not tested here: it needs
//! Valgrind, which CI does not install.
//!
//! Ignored by a plain `cargo test`, because they need a compositor
//! installed; they start only isolated sessions of their own, so they are
//! safe to run anywhere. Name the compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod measure {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output, Stdio};

    use rustix::process::{Pid, test_kill_process};

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// `xtask measure` of `sh -c script`: one run, short windows.
    fn measure(options: &[&str], script: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .args(["--settle", "1", "--idle", "2"])
            .args(options)
            .args(["sh", "-c", script])
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// A script line that records, in `dir`, the stand-in's own pid and its
    /// session's directory; with `child`, it also starts a background
    /// process for the session to end, and records its pid.
    fn record(dir: &Path, child: bool) -> String {
        let dir = dir.display();
        let background = if child {
            format!("sleep 600 & echo $! > {dir}/child; ")
        } else {
            String::new()
        };
        format!(
            "echo $$ > {dir}/pid; echo \"$DESKRINCH_SESSION_DIR\" > {dir}/session; {background}"
        )
    }

    /// Asserts that nothing the stand-in recorded in `dir` outlived the
    /// measurement: its processes have ended and its session's directory is
    /// gone.
    fn assert_session_ended(dir: &Path) {
        for name in ["pid", "child"] {
            let Ok(text) = fs::read_to_string(dir.join(name)) else {
                assert_eq!(name, "child", "the stand-in recorded no pid");
                continue;
            };
            let pid = Pid::from_raw(text.trim().parse().unwrap()).unwrap();
            assert!(test_kill_process(pid).is_err(), "{name} {pid:?} still runs");
        }
        let session = fs::read_to_string(dir.join("session")).unwrap();
        let session = session.trim();
        assert!(session.starts_with("/tmp/deskrinch-session-"), "{session}");
        assert!(!Path::new(session).exists(), "{session} was not removed");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn reports_start_up_memory_and_idle() {
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "{}echo '[PERF] frame 1: style 0.10ms' >&2; \
             while :; do sleep 0.2; echo '[PERF] frame 2: paint_frames=1' >&2; done",
            record(dir.path(), true)
        );
        let output = measure(&[], &script);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        for section in [
            // Start-up.
            "CPU to first frame, user+system",
            "  thread sh",
            "threads started by the first frame",
            // Memory.
            "VmRSS (settled)",
            "Pss (settled)",
            "  Rss: ",
            // Idle.
            "frames in the idle window",
            "idle CPU per minute",
            "wake-ups per second (voluntary switches)",
            "CPU per frame in the window (with idle)",
            // Apart, and only as context.
            "Wall-clock time: NOT comparable",
            "wall time to first frame",
            "load average at the first frame",
            "renderer: software (no GPU adapter logged)",
        ] {
            assert!(stdout.contains(section), "no {section:?} in:\n{stdout}");
        }
        assert_session_ended(dir.path());
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_program_that_exits_before_its_first_frame_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        // No background process here: one would hold the output open, and
        // the stand-in's last line would then be read only after a grace
        // period.
        let script = format!(
            "{}echo 'cannot open the display' >&2; exit 3",
            record(dir.path(), false)
        );
        let output = measure(&[], &script);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains("while it was measured"), "{stderr}");
        assert!(stderr.contains("cannot open the display"), "{stderr}");
        assert_session_ended(dir.path());
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_program_that_never_draws_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "{}echo 'waiting for nothing' >&2; exec sleep 600",
            record(dir.path(), true)
        );
        let output = measure(&["--first-frame-timeout", "3"], &script);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains("presented no frame within 3 s"), "{stderr}");
        assert!(stderr.contains("waiting for nothing"), "{stderr}");
        assert_session_ended(dir.path());
    }

    /// The stand-in's summon and dismiss: scripts in `dir` that `sh` runs
    /// when the measurement runs the program (`sh`) with their paths. The
    /// summon asks the stand-in to draw, through a file it watches.
    fn summons(dir: &Path) -> (String, String) {
        let summon = dir.join("summon.sh");
        fs::write(&summon, format!("touch {}/show\n", dir.display())).unwrap();
        let dismiss = dir.join("dismiss.sh");
        fs::write(&dismiss, "true\n").unwrap();
        (summon.display().to_string(), dismiss.display().to_string())
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn reports_each_show() {
        let dir = tempfile::tempdir().unwrap();
        let (summon, dismiss) = summons(dir.path());
        let shown = dir.path().join("show");
        let script = format!(
            "{}n=0; while :; do if [ -e {flag} ]; then rm {flag}; n=$((n + 1)); \
             echo \"[PERF] frame $n: paint_frames=1\" >&2; fi; sleep 0.05; done",
            record(dir.path(), false),
            flag = shown.display()
        );
        let output = measure(
            &["--summon", &summon, "--dismiss", &dismiss, "--shows", "2"],
            &script,
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        for section in [
            "shows: CPU from each summon to its first frame",
            "show 1: CPU from the summon to its first frame",
            "show 2: CPU from the summon to its first frame",
            "show 1: wall time",
            "load average after the shows",
        ] {
            assert!(stdout.contains(section), "no {section:?} in:\n{stdout}");
        }
        assert!(!stdout.contains("idle CPU per minute"), "{stdout}");
        assert_session_ended(dir.path());
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_program_shown_before_its_summon_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let (summon, dismiss) = summons(dir.path());
        let script = format!(
            "{}echo '[PERF] frame 1: style 0.10ms' >&2; exec sleep 600",
            record(dir.path(), false)
        );
        let output = measure(&["--summon", &summon, "--dismiss", &dismiss], &script);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains("before it was summoned"), "{stderr}");
        assert_session_ended(dir.path());
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_state_is_seeded_before_the_program_starts() {
        let dir = tempfile::tempdir().unwrap();
        let seed = dir.path().join("seed");
        fs::create_dir_all(seed.join("deskrinch")).unwrap();
        let record = "version = 1\n[[start]]\nid = \"a.desktop\"\nscore = 2.0\nlast = 5\n";
        fs::write(seed.join("deskrinch/starts.toml"), record).unwrap();
        // The stand-in copies what it finds at its start, then draws.
        let script = format!(
            "{}cp \"$XDG_STATE_HOME/deskrinch/starts.toml\" {found}; \
             echo '[PERF] frame 1: style 0.10ms' >&2; exec sleep 600",
            record(dir.path(), false),
            found = dir.path().join("found").display()
        );
        let seed = seed.display().to_string();
        let output = measure(&["--state", &seed], &script);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert_eq!(fs::read_to_string(dir.path().join("found")).unwrap(), record);
        let note = format!("state seeded from {seed}");
        assert!(stdout.contains(&note), "no {note:?} in:\n{stdout}");
        assert_session_ended(dir.path());
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_state_that_is_not_a_directory_fails_before_any_session() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        fs::write(&file, "").unwrap();
        for state in [file, dir.path().join("missing")] {
            let started = dir.path().join("started");
            let script = format!("touch {}; exec sleep 600", started.display());
            let output = measure(&["--state", &state.display().to_string()], &script);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(!output.status.success(), "{stderr}");
            assert!(stderr.contains("read the state to seed"), "{stderr}");
            assert!(!started.exists(), "the program was started");
        }
    }
}

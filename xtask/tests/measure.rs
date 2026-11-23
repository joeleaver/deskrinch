//! `cargo xtask measure` end to end, on stand-ins for a rinch program: shell
//! scripts that print rinch's `[PERF] frame N:` lines (or do not), in the
//! isolated sessions the task starts. What is checked is the orchestration:
//! the report's sections, the errors for a program that exits or never draws,
//! and that each session is ended afterwards. Nothing here asserts on time.
//!
//! The Callgrind path (`--instructions`) is tested apart, in
//! `tests/measure_callgrind.rs`: it needs Valgrind.
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
mod fifo;
#[cfg(test)]
mod report;
#[cfg(test)]
mod stand_in;

#[cfg(test)]
mod measure {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output, Stdio};

    use deskrinch_test_session::SESSION_DIR_VARIABLE;
    use rustix::process::{Pid, test_kill_process};

    use crate::report::number_after;
    use crate::stand_in::{summoned, summons};

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

    /// The note of what the stand-in, `sh`, was granted under the compositor
    /// tested with: under KWin, which restricts interfaces, the warning that
    /// `sh` is none of the shell's components; elsewhere, that the
    /// compositor restricts nothing.
    fn granted_to_sh() -> String {
        match compositor().as_str() {
            "kwin" => "granted: NOTHING: sh is not one of the shell's components by its file \
                       name; if it is one, --component PACKAGE names it, and it is granted as \
                       the shell grants it"
                .to_owned(),
            other => format!("granted: nothing: {other} restricts nothing"),
        }
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
            "echo $$ > {dir}/pid; echo \"${SESSION_DIR_VARIABLE}\" > {dir}/session; {background}"
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
            // Each run's notes.
            "idle wake-ups by thread: ",
            "the program's output: ",
        ] {
            assert!(stdout.contains(section), "no {section:?} in:\n{stdout}");
        }
        let granted = granted_to_sh();
        assert!(stdout.contains(&granted), "no {granted:?} in:\n{stdout}");
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

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn reports_each_show() {
        let dir = tempfile::tempdir().unwrap();
        let stand_in = summoned(dir.path(), false);
        let (summon, dismiss) = (stand_in.summon, stand_in.dismiss);
        let script = format!("{}{}", record(dir.path(), false), stand_in.script);
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
    fn waits_for_preparations_and_reports_them_and_the_hidden_memory() {
        // The stand-in prepares once at start; after each dismiss (an `h` in
        // the FIFO it reads, blocked) it prepares six times, half a second
        // apart, sleeping in between, so its threads stay quiet. A settle that waited only for
        // quiet threads would summon the next show after a second, with one
        // or two of the six done; one that also waits for no prepared frame
        // summons it after all six. It starts quiet, sleeping three seconds
        // before its first preparation, as a stand-in starved of CPU on a
        // loaded machine does. A sleep and not a count: only an idle start
        // passes a quiet window (a count runs, and keeps the settle
        // waiting), and two windows of idleness against a wait of one
        // summon it before it prepared every time, not by luck, unless
        // `--prepares` makes the first show wait for the preparation.
        let dir = tempfile::tempdir().unwrap();
        let (summon, _) = summons(dir.path());
        let asked = dir.path().join("asked");
        let dismiss = dir.path().join("dismiss.sh");
        fs::write(&dismiss, format!("printf 'h\\n' > {}\n", asked.display())).unwrap();
        let script = format!(
            "{}sleep 3; echo '[PERF] prepared 1: a' >&2; exec 3<>{asked}; n=0; p=1; \
             while read asked <&3; do case $asked in \
             s) n=$((n + 1)); echo \"[PERF] frame $n: paint_frames=1\" >&2 ;; \
             h) for k in 1 2 3 4 5 6; do sleep 0.5; \
             p=$((p + 1)); echo \"[PERF] prepared $p: a\" >&2; done ;; \
             esac; done",
            record(dir.path(), false),
            asked = asked.display(),
        );
        let dismiss = dismiss.display().to_string();
        let output = measure(
            &[
                "--summon",
                &summon,
                "--dismiss",
                &dismiss,
                "--shows",
                "2",
                "--prepares",
            ],
            &script,
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        let first = number_after(&stdout, "show 1: prepared frames before it, in all");
        assert_eq!(first, 1, "{stdout}");
        let second = number_after(&stdout, "show 2: prepared frames before it, in all");
        assert_eq!(
            second, 7,
            "the second show came before the six preparations:\n{stdout}"
        );
        // The memory, read once, hidden and settled, before the first show.
        for row in ["VmRSS (settled)", "Pss (settled)", "  Rss: "] {
            assert!(stdout.contains(row), "no {row:?} in:\n{stdout}");
        }
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

    /// A seeded directory: the option, the session's variable it seeds,
    /// what the report calls it, and the file a test seeds it with.
    const SEEDS: [(&str, &str, &str, &str); 2] = [
        (
            "--state",
            "XDG_STATE_HOME",
            "state",
            "deskrinch/starts.toml",
        ),
        (
            "--config",
            "XDG_CONFIG_HOME",
            "configuration",
            "deskrinch/config.toml",
        ),
    ];

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_state_and_the_configuration_are_seeded_before_the_program_starts() {
        for (option, variable, name, file) in SEEDS {
            let dir = tempfile::tempdir().unwrap();
            let seed = dir.path().join("seed");
            fs::create_dir_all(seed.join("deskrinch")).unwrap();
            let seeded = format!("# seeded by {option}\nversion = 1\n");
            fs::write(seed.join(file), &seeded).unwrap();
            // The stand-in copies what it finds at its start, then draws.
            let script = format!(
                "{}cp \"${variable}/{file}\" {found}; \
                 echo '[PERF] frame 1: style 0.10ms' >&2; exec sleep 600",
                record(dir.path(), false),
                found = dir.path().join("found").display()
            );
            let seed_arg = seed.display().to_string();
            let output = measure(&[option, &seed_arg], &script);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success(), "{option}: {stdout}\n{stderr}");
            assert_eq!(
                fs::read_to_string(dir.path().join("found")).unwrap(),
                seeded,
                "{option}"
            );
            let note = format!("{name} ({variable}) seeded from {seed_arg}: {file}");
            assert!(stdout.contains(&note), "no {note:?} in:\n{stdout}");
            // Nothing is written back.
            assert_eq!(fs::read_to_string(seed.join(file)).unwrap(), seeded);
            assert_eq!(fs::read_dir(seed.join("deskrinch")).unwrap().count(), 1);
            assert_session_ended(dir.path());
        }
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_seed_that_is_not_a_directory_fails_before_any_session() {
        for (option, _, name, _) in SEEDS {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("file");
            fs::write(&file, "").unwrap();
            for seed in [file, dir.path().join("missing")] {
                let started = dir.path().join("started");
                let script = format!("touch {}; exec sleep 600", started.display());
                let output = measure(&[option, &seed.display().to_string()], &script);
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(!output.status.success(), "{option}: {stderr}");
                let refusal = format!("read the {name} to seed");
                assert!(stderr.contains(&refusal), "{stderr}");
                assert!(!started.exists(), "the program was started");
            }
        }
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_window_opened_for_the_idle_window_is_named_and_counted() {
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "{}echo '[PERF] frame 1: style 0.10ms' >&2; exec sleep 600",
            record(dir.path(), false)
        );
        let output = measure(&["--window", "org.deskrinch.MeasuredWindow"], &script);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        for section in [
            "window: a window with app id org.deskrinch.MeasuredWindow opened at the idle \
             window's start",
            "wake-ups in the idle window (voluntary switches)",
            "frames in the idle window",
        ] {
            assert!(stdout.contains(section), "no {section:?} in:\n{stdout}");
        }
        assert_session_ended(dir.path());
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_window_gone_before_the_idle_window_ends_fails_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let compositor = match compositor().as_str() {
            "kwin" => "kwin_wayland",
            other => other,
        }
        .to_owned();
        // After its first frame the stand-in ends its own session's
        // compositor (the session's processes share one group), so the
        // window's connection ends inside the idle window.
        let script = format!(
            "{}echo '[PERF] frame 1: style 0.10ms' >&2; sleep 1; \
             kill $(pgrep -g $(ps -o pgid= $$ | tr -d ' ') -x {compositor}); exec sleep 600",
            record(dir.path(), false)
        );
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args([
                "measure",
                "--compositor",
                &self::compositor(),
                "--runs",
                "1",
            ])
            .args(["--settle", "0", "--idle", "4"])
            .args(["--window", "org.deskrinch.MeasuredWindow"])
            .args(["sh", "-c", &script])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
        assert!(
            stderr.contains("was gone by the end of the idle window"),
            "{stderr}"
        );
        assert!(
            !stdout.contains("idle CPU per minute"),
            "a report was printed for a run without its window:\n{stdout}"
        );
        assert_session_ended(dir.path());
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_environment_asked_for_reaches_the_program_and_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let found = dir.path().join("found");
        let script = format!(
            "{}echo \"$MEASURE_ASKED\" > {}; echo '[PERF] frame 1: style 0.10ms' >&2; \
             exec sleep 600",
            record(dir.path(), false),
            found.display()
        );
        let output = measure(&["--env", "MEASURE_ASKED=a=b"], &script);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert_eq!(fs::read_to_string(&found).unwrap(), "a=b\n");
        assert!(
            stdout.contains("Environment set: MEASURE_ASKED=a=b"),
            "{stdout}"
        );
        assert!(
            stdout.contains("environment set: MEASURE_ASKED=a=b"),
            "{stdout}"
        );
        assert_session_ended(dir.path());
    }

    /// A stand-in that logs an error as a component does, then draws.
    fn logs_an_error(dir: &Path) -> String {
        format!(
            "{}echo '2026-11-13T21:18:32Z ERROR deskrinch_component::report: the taskbar shows \
             none' >&2; echo '[PERF] frame 1: style 0.10ms' >&2; exec sleep 600",
            record(dir, false)
        )
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_program_that_logged_an_error_is_not_measured() {
        let dir = tempfile::tempdir().unwrap();
        let output = measure(&[], &logs_an_error(dir.path()));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
        assert!(stderr.contains("logged 1 error(s)"), "{stderr}");
        assert!(stderr.contains("the taskbar shows none"), "{stderr}");
        assert!(stderr.contains("--allow-errors"), "{stderr}");
        assert!(
            !stdout.contains("idle CPU per minute"),
            "a report was printed for it:\n{stdout}"
        );
        assert_session_ended(dir.path());
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn allowed_errors_are_measured_and_noted() {
        let dir = tempfile::tempdir().unwrap();
        let output = measure(&["--allow-errors"], &logs_an_error(dir.path()));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(stdout.contains("idle CPU per minute"), "{stdout}");
        assert!(
            stdout.contains("errors the program logged (1): "),
            "{stdout}"
        );
        assert!(stdout.contains("the taskbar shows none"), "{stdout}");
        assert_session_ended(dir.path());
    }
}

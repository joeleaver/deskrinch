//! `cargo xtask measure --step-command` end to end, on stand-ins: `sh`
//! scripts that print rinch's frame and window lines when a file they watch
//! appears, and step commands that make that file.
//!
//! Ignored by a plain `cargo test`, because they need a compositor
//! installed; they start only isolated sessions of their own. Name the
//! compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure_command_steps -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod file_driven;
#[cfg(test)]
mod report;

#[cfg(test)]
mod command_steps {
    use std::process::{Command, Output, Stdio};

    use crate::file_driven::{MAIN_FRAME, go, prints, stand_in};
    use crate::report::number_after;

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// `xtask measure` with `options`, of `sh -c script`: one run, short
    /// windows.
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

    /// `output`'s report, after asserting the measurement succeeded.
    fn succeeded(output: &Output) -> String {
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        stdout
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_step_that_opens_a_window_and_one_that_closes_it_are_each_measured() {
        let dir = tempfile::tempdir().unwrap();
        let script = stand_in(
            dir.path(),
            MAIN_FRAME,
            &[
                &prints(
                    "'[PERF] window 2 opened: title=\"popup\" app_id=\"x\" output=\"V-0\" \
                     output_id=1' '[PERF] window 2 frame 1: style 0.10ms | signal_notifies=1'",
                ),
                &prints("'[PERF] window 2 closed'"),
            ],
        );
        let (open, close) = (go(dir.path(), 1), go(dir.path(), 2));
        let output = measure(
            &["--step-command", &open, "--step-command", &close],
            &script,
        );
        let report = succeeded(&output);
        for wanted in [
            "Command steps (after the idle window, each to settled): 1.",
            "command step 1 of 2: CPU from the command to settled",
            "command step 2 of 2: CPU from the command to settled",
            "commands, the steps' sum: CPU from the command to settled",
            "command step 1 of 2: windows: opened window 2 (\"popup\" on V-0)",
            "command step 2 of 2: windows: closed window 2",
        ] {
            assert!(report.contains(wanted), "no {wanted:?} in:\n{report}");
        }
        // Each step's wall time to the frame it led to; the closing led to
        // none, and says so rather than printing a zero.
        let (_, wall) = report
            .split_once("Wall-clock time")
            .expect("a wall section");
        let row = |step: &str| {
            let label = format!("{step}: wall time from the command to its first frame");
            wall.lines()
                .find(|line| line.contains(&label))
                .unwrap_or_else(|| panic!("no {label:?} in:\n{report}"))
                .to_owned()
        };
        assert!(row("command step 1 of 2").contains(" ms"), "{report}");
        let closed = row("command step 2 of 2");
        assert!(
            closed.contains("no frame") && !closed.contains("0.0 ms"),
            "{report}"
        );
        assert_eq!(
            number_after(&report, "command step 1 of 2: frames presented"),
            1
        );
        assert_eq!(
            number_after(&report, "command step 1 of 2: of them with reactive work"),
            1
        );
        // Closing a window is a change, frame or none.
        assert_eq!(
            number_after(&report, "command step 2 of 2: frames presented"),
            0
        );
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_step_counts_the_frames_of_every_window() {
        let dir = tempfile::tempdir().unwrap();
        let start = "'[PERF] window 1 opened: title=\"w\" app_id=\"x\" output=\"V-0\" output_id=1' \
                     '[PERF] window 2 opened: title=\"w\" app_id=\"x\" output=\"V-1\" output_id=2' \
                     '[PERF] window 1 frame 1: style 0.10ms' '[PERF] window 2 frame 1: style 0.10ms'";
        let script = stand_in(
            dir.path(),
            start,
            &[&prints(
                "'[PERF] window 1 frame 2: style 0.10ms' '[PERF] window 2 frame 2: style 0.10ms'",
            )],
        );
        let repaint = go(dir.path(), 1);
        let output = measure(&["--windows", "2", "--step-command", &repaint], &script);
        let report = succeeded(&output);
        assert_eq!(
            number_after(&report, "command step 1 of 1: frames presented"),
            2
        );
        assert!(
            report.contains("command step 1 of 1: windows: none opened or closed"),
            "{report}"
        );
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_command_s_own_cpu_is_not_counted() {
        let dir = tempfile::tempdir().unwrap();
        let script = stand_in(
            dir.path(),
            MAIN_FRAME,
            &[&prints("'[PERF] frame 2: style 0.10ms'")],
        );
        // A second of the command's own CPU, then the stand-in's frame.
        let spin = format!(
            "timeout 1 sh -c 'while :; do :; done'; {}",
            go(dir.path(), 1)
        );
        let output = measure(&["--step-command", &spin], &script);
        let report = succeeded(&output);
        let cpu = number_after(
            &report,
            "command step 1 of 1: CPU from the command to settled",
        );
        assert!(cpu < 500, "the step's CPU is {cpu} ms:\n{report}");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_command_that_fails_fails_the_run_with_its_output() {
        let dir = tempfile::tempdir().unwrap();
        let script = stand_in(dir.path(), MAIN_FRAME, &[]);
        let output = measure(&["--step-command", "echo cannot; exit 3"], &script);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(stderr.contains("exit status: 3"), "{stderr}");
        assert!(stderr.contains("cannot"), "{stderr}");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_step_after_which_nothing_changes_fails_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let script = stand_in(dir.path(), MAIN_FRAME, &[]);
        let output = measure(
            &["--first-frame-timeout", "3", "--step-command", "true"],
            &script,
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains("showed no change for command step 1 of 1"),
            "{stderr}"
        );
    }

    /// Plain `cargo test` runs it: refused before any session.
    #[test]
    fn a_command_step_with_summon_is_refused() {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--step-command", "true", "--summon", "show"])
            .args(["--dismiss", "hide", "sh", "-c", "exit 0"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(stderr.contains("cannot be used with"), "{stderr}");
    }
}

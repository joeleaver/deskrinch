//! `cargo xtask measure`'s state made before the settle and its readings
//! after the steps, end to end, on stand-ins (`sh` scripts that print
//! rinch's lines, and grow, when a file they watch appears):
//! `--setup-command`, `--memory-after-steps`, the renderer read over the
//! whole run, and the warm cache's filling start running the commands.
//!
//! Ignored by a plain `cargo test`, because they need a compositor
//! installed; they start only isolated sessions of their own. Name the
//! compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure_setup -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod file_driven;
#[cfg(test)]
mod report;

#[cfg(test)]
mod setup {
    use std::fs;
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

    /// The snippet that makes the stand-in itself hold 64 MiB more: a shell
    /// variable of that many bytes, in the stand-in's own process.
    const GROW: &str = "big=$(head -c 67108864 /dev/zero | tr '\\0' a)";

    /// The snippet that starts a frame every 0.2 s from then on, on a
    /// process of its own (`true` after it, so that the `;` the stand-in
    /// puts after a snippet follows a command, not the `&`).
    const KEEP_DRAWING: &str = "(n=1; while :; do n=$((n + 1)); \
                                printf '[PERF] frame %s: style 0.10ms\\n' $n >&2; sleep 0.2; done) \
                                & true";

    /// A popup's window opened and its first frame, rinch saying it presents
    /// with its software renderer.
    const POPUP: &str = "'rinch: presenting with the software renderer' \
                         '[PERF] window 2 opened: title=\"popup\" app_id=\"x\" output=\"V-0\" \
                         output_id=1' '[PERF] window 2 frame 1: style 0.10ms'";

    /// Fails against a setup command run after the idle window, or not at
    /// all: the memory reading and the idle window are then the stand-in's
    /// before it grew and drew.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_setup_command_s_effect_is_in_the_memory_reading_and_the_idle_window() {
        let dir = tempfile::tempdir().unwrap();
        let script = stand_in(
            dir.path(),
            MAIN_FRAME,
            &[&format!("{GROW}; {KEEP_DRAWING}")],
        );
        let setup = go(dir.path(), 1);
        let output = measure(&["--setup-command", &setup], &script);
        let report = succeeded(&output);
        assert!(
            report.contains(
                "Setup commands (after start-up's end, before the settle, each to settled, not \
                 measured): 1."
            ),
            "{report}"
        );
        assert!(
            report.contains("setup command 1 of 1: windows: none opened or closed"),
            "{report}"
        );
        let rss = number_after(&report, "VmRSS (settled)");
        assert!(rss >= 64, "the stand-in held {rss} MiB settled:\n{report}");
        let frames = number_after(&report, "frames in the idle window");
        assert!(frames > 0, "no frame in the idle window:\n{report}");
    }

    /// Fails against a failure that names the command as a step's, or none.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_failing_setup_command_fails_the_run_and_says_which() {
        let dir = tempfile::tempdir().unwrap();
        let script = stand_in(dir.path(), MAIN_FRAME, &[]);
        let output = measure(
            &[
                "--setup-command",
                "true",
                "--setup-command",
                "echo cannot; exit 3",
            ],
            &script,
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains("setup command 2 of 2 \"echo cannot; exit 3\" ended exit status: 3"),
            "{stderr}"
        );
        assert!(stderr.contains("cannot"), "{stderr}");
    }

    /// Fails against the renderer read at start-up's end, when nothing had
    /// presented yet: the check would then fail a program that presented
    /// with the renderer expected, in a step.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_renderer_is_checked_when_the_first_frame_comes_in_a_step() {
        let dir = tempfile::tempdir().unwrap();
        let script = stand_in(dir.path(), "'up'", &[&prints(POPUP)]);
        let show = go(dir.path(), 1);
        let output = measure(
            &[
                "--start-ends",
                "line:up",
                "--expect-renderer",
                "software",
                "--step-command",
                &show,
            ],
            &script,
        );
        let report = succeeded(&output);
        assert!(
            report.contains("every run presented with the Software renderer, as expected"),
            "{report}"
        );
        assert!(
            report.contains("renderer: software (no GPU adapter logged)"),
            "{report}"
        );
        assert!(!report.contains("No window:"), "{report}");
    }

    /// Fails against the old wording, "it did not say which renderer
    /// presented", for a run in which nothing presented.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_run_in_which_nothing_presented_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let script = stand_in(dir.path(), "'up'", &[]);
        let output = measure(
            &["--start-ends", "line:up", "--expect-renderer", "software"],
            &script,
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stdout}\n{stderr}");
        assert!(
            stderr.contains("run 1: nothing presented in this run"),
            "{stderr}"
        );
        assert!(
            stdout.contains("renderer: nothing presented in this run"),
            "{stdout}"
        );
    }

    /// Fails against a second reading taken before the steps (or none): it
    /// would then not hold what the step made the stand-in hold.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn memory_after_the_steps_is_read_after_them() {
        let dir = tempfile::tempdir().unwrap();
        let script = stand_in(
            dir.path(),
            MAIN_FRAME,
            &[&format!("{GROW}; {}", prints(POPUP))],
        );
        let step = go(dir.path(), 1);
        let output = measure(&["--memory-after-steps", "--step-command", &step], &script);
        let report = succeeded(&output);
        let before = number_after(&report, "VmRSS (settled)");
        let after = number_after(&report, "VmRSS (after the steps, settled)");
        assert!(
            before < 32 && after >= 64,
            "settled {before} MiB, after the steps {after} MiB:\n{report}"
        );
        // After the steps' rows.
        let steps = report.find("commands: frames presented").unwrap();
        let reading = report.find("VmRSS (after the steps, settled)").unwrap();
        assert!(steps < reading, "{report}");
    }

    /// Fails against a filling start that does not run the commands: each
    /// would then have run once, in the measured run alone.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_warm_filling_start_runs_the_setup_commands_and_the_steps() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("ran.log");
        let script = stand_in(
            dir.path(),
            MAIN_FRAME,
            &[&prints("'[PERF] frame 2: style 0.10ms'")],
        );
        let setup = format!("echo setup >> {}", log.display());
        let step = format!("echo step >> {}; {}", log.display(), go(dir.path(), 1));
        let output = measure(
            &[
                "--shader-cache",
                "warm",
                "--setup-command",
                &setup,
                "--step-command",
                &step,
            ],
            &script,
        );
        succeeded(&output);
        let ran = fs::read_to_string(&log).unwrap();
        let count = |what: &str| ran.lines().filter(|line| *line == what).count();
        assert_eq!(
            (count("setup"), count("step")),
            (2, 2),
            "the filling start and the run each ran them once:\n{ran}"
        );
    }

    /// Plain `cargo test` runs it: refused before any session, saying why.
    #[test]
    fn a_setup_command_with_summon_is_refused_saying_why() {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--setup-command", "true", "--summon", "show"])
            .args(["--dismiss", "hide", "sh", "-c", "exit 0"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains("--setup-command cannot be used with --summon")
                && stderr.contains("no settle or idle window"),
            "{stderr}"
        );
    }

    /// Plain `cargo test` runs it: refused before any session, saying why.
    #[test]
    fn memory_after_the_steps_without_steps_is_refused() {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--memory-after-steps", "sh", "-c", "exit 0"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains("--memory-after-steps needs steps to read after"),
            "{stderr}"
        );
    }
}

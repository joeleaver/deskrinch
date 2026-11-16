//! `cargo xtask measure --start-ends` end to end, on stand-ins: `sh`
//! scripts that print rinch's frame lines and a line of their own, and, for
//! an end on the session's bus, this test binary run again as the measured
//! program, owning a name ([`start_ends::bus_owner`]).
//!
//! Ignored by a plain `cargo test`, because they need a compositor
//! installed; they start only isolated sessions of their own. Name the
//! compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure_start_ends -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod report;

#[cfg(test)]
mod start_ends {
    use std::process::{Command, Output, Stdio};
    use std::time::Duration;

    use crate::report::number_after;

    /// The variable naming the bus name the stand-in owns.
    const OWN: &str = "DESKRINCH_TEST_OWN_NAME";

    /// How long the scripts wait between their first frame and what ends
    /// their start-up, in milliseconds: an end at the first frame would come
    /// well before it, on any machine.
    const LATER_MS: u64 = 1500;

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// `xtask measure` with `options`, then `program`: one run, short
    /// windows.
    fn measure(options: &[&str], program: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .args(["--settle", "1", "--idle", "2"])
            .args(options)
            .args(program)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// A script that presents a first frame, then, `LATER_MS` later,
    /// prints `then` (a second frame, or a line), and stays.
    fn script(then: &str) -> String {
        format!(
            "echo '[PERF] frame 1: style 0.10ms' >&2; sleep {}.{:03}; echo '{then}' >&2; exec sleep 600",
            LATER_MS / 1000,
            LATER_MS % 1000
        )
    }

    /// `output`'s report, after asserting the measurement succeeded.
    fn succeeded(output: &Output) -> String {
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        stdout
    }

    #[track_caller]
    fn ended_later(report: &str, end: &str) {
        let header = format!("Start-up ends at: {end} (--start-ends)");
        assert!(report.contains(&header), "no {header:?} in:\n{report}");
        assert!(
            report.contains("CPU to start-up's end, user+system"),
            "{report}"
        );
        assert!(!report.contains("CPU to first frame"), "{report}");
        let wall = number_after(report, "wall time to start-up's end");
        assert!(wall >= LATER_MS - 100, "ended at {wall} ms:\n{report}");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn start_up_ends_at_the_nth_frame() {
        let output = measure(
            &["--start-ends", "frame:2"],
            &["sh", "-c", &script("[PERF] frame 2: style 0.10ms")],
        );
        ended_later(&succeeded(&output), "the main window's 2nd frame");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn start_up_ends_at_the_first_frame_that_carries_a_counter() {
        let output = measure(
            &["--start-ends", "counter:image_draws"],
            &[
                "sh",
                "-c",
                &script("[PERF] frame 2: style 0.10ms | image_draws=1"),
            ],
        );
        ended_later(
            &succeeded(&output),
            "the main window's first frame with image_draws",
        );
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_counter_that_no_frame_carries_fails_the_run_naming_it() {
        let output = measure(
            &[
                "--start-ends",
                "counter:image_draws",
                "--first-frame-timeout",
                "4",
            ],
            &["sh", "-c", &script("[PERF] frame 2: style 0.10ms | x=1")],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains(
                "did not reach its end (the main window's first frame with image_draws) within 4 s"
            ),
            "{stderr}"
        );
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn start_up_ends_at_a_line_the_program_writes() {
        let output = measure(
            &["--start-ends", "line:all set"],
            &["sh", "-c", &script("all set, at last")],
        );
        ended_later(
            &succeeded(&output),
            "the first line of its output with \"all set\"",
        );
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn an_end_that_never_comes_fails_the_run_naming_it() {
        let output = measure(
            &["--start-ends", "frame:3", "--first-frame-timeout", "4"],
            &["sh", "-c", &script("[PERF] frame 2: style 0.10ms")],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains("did not reach its end (the main window's 3rd frame) within 4 s"),
            "{stderr}"
        );
    }

    /// This test binary, run as the measured program, as the stand-in
    /// that owns a bus name.
    fn owner() -> Vec<String> {
        let exe = std::env::current_exe().unwrap();
        vec![
            exe.display().to_string(),
            "--exact".into(),
            "start_ends::bus_owner".into(),
            "--ignored".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ]
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn start_up_ends_owning_a_bus_name_and_a_program_with_no_window_is_said_so() {
        let own = format!("{OWN}=org.deskrinch.MeasureStandIn");
        let owner = owner();
        let program: Vec<&str> = owner.iter().map(String::as_str).collect();
        let output = measure(
            &[
                "--start-ends",
                "bus-name:org.deskrinch.MeasureStandIn",
                "--env",
                &own,
            ],
            &program,
        );
        let report = succeeded(&output);
        let header = "Start-up ends at: owning org.deskrinch.MeasureStandIn on the session's bus";
        assert!(report.contains(header), "no {header:?} in:\n{report}");
        assert!(report.contains("No window:"), "{report}");
        assert!(report.contains("CPU to start-up's end"), "{report}");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn ready_is_the_shell_s_readiness_for_the_component() {
        let own = format!("{OWN}=org.deskrinch.Launcher");
        let owner = owner();
        let program: Vec<&str> = owner.iter().map(String::as_str).collect();
        let output = measure(
            &[
                "--start-ends",
                "ready",
                "--component",
                "deskrinch-launcher",
                "--env",
                &own,
            ],
            &program,
        );
        let report = succeeded(&output);
        let header = "owning org.deskrinch.Launcher on the session's bus (the readiness \
                      deskrinch-shell declares for deskrinch-launcher)";
        assert!(report.contains(header), "no {header:?} in:\n{report}");
    }

    /// Plain `cargo test` runs it: both are refused before any session.
    #[test]
    fn ready_for_a_component_ready_once_started_and_an_end_with_summon_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let started = dir.path().join("started");
        let touch = format!("touch {}", started.display());
        let refused = |options: &[&str]| {
            let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
                .arg("measure")
                .args(options)
                .args(["sh", "-c", &touch])
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(!output.status.success());
            String::from_utf8_lossy(&output.stderr).into_owned()
        };
        let said = refused(&[
            "--start-ends",
            "ready",
            "--component",
            "deskrinch-wallpaper",
        ]);
        assert!(said.contains("ready once started"), "{said}");
        let said = refused(&[
            "--start-ends",
            "frame:2",
            "--summon",
            "show",
            "--dismiss",
            "hide",
        ]);
        assert!(said.contains("cannot be used with"), "{said}");
        assert!(!started.exists(), "the program was started");
    }

    /// The stand-in: owns the name `OWN` names on the session's bus, says
    /// so, and stays; without the variable (a plain `--ignored` run) it does
    /// nothing.
    #[test]
    #[ignore = "the stand-in measured by this file's tests"]
    fn bus_owner() {
        let Ok(name) = std::env::var(OWN) else {
            return;
        };
        let connection = zbus::block_on(async {
            let connection = zbus::Connection::session().await.unwrap();
            connection.request_name(name.as_str()).await.unwrap();
            connection
        });
        eprintln!("owning {name}");
        std::thread::sleep(Duration::from_secs(600));
        drop(connection);
    }
}

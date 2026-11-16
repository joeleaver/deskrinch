//! `cargo xtask measure --place-pointer --move-pointer` end to end, on
//! stand-ins for a rinch program in isolated sessions, the pointer moved by
//! the real input client (`cargo xtask input`). The stand-ins hear nothing:
//! one presents a frame every 0.3 s, so that each step is followed by one,
//! and one presents nothing after its first. What is checked is the
//! orchestration: each step measured after the idle window (the `--window`
//! window still open) or after each show, and each way such a run fails.
//! That the client's lines reach a window is `tests/input_client.rs`'s.
//!
//! Ignored by a plain `cargo test`; name the compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure_pointer -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod report;
#[cfg(test)]
mod stand_in;

#[cfg(test)]
mod measure_pointer {
    use std::process::{Command, Output, Stdio};

    use crate::report::number_after;
    use crate::stand_in::summoned;

    /// A stand-in that presents its first frame, then one every 0.3 s.
    const DRAWS: &str = "echo '[PERF] frame 1: style 0.10ms' >&2; n=1; \
                         while :; do sleep 0.3; n=$((n + 1)); \
                         echo \"[PERF] frame $n: paint_frames=1\" >&2; done";
    /// A stand-in that presents its first frame and nothing more.
    const STILL: &str = "echo '[PERF] frame 1: style 0.10ms' >&2; exec sleep 600";

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// `xtask measure` of `sh -c script`: one run, short windows, and
    /// `options` besides.
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

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn each_step_is_measured_after_the_idle_window_the_window_still_open() {
        let output = measure(
            &[
                "--outputs",
                "2",
                "--window",
                "org.deskrinch.PointerStandIn",
                "--place-pointer",
                "640,360",
                "--move-pointer",
                "600,300",
                "--move-pointer",
                "2:10,10",
            ],
            DRAWS,
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        for row in [
            "pointer step 1 of 2 (to 1:600,300): CPU from the move to settled",
            "pointer step 2 of 2 (to 2:10,10): CPU from the move to settled",
            "pointer, the steps' sum: CPU from the move to settled",
            "frames in the idle window",
            "Pointer: placed at 1:640,360, then 2 measured step(s)",
            "a window with app id org.deskrinch.PointerStandIn",
        ] {
            assert!(stdout.contains(row), "no {row:?} in:\n{stdout}");
        }
        for step in ["1 of 2 (to 1:600,300)", "2 of 2 (to 2:10,10)"] {
            let frames = number_after(&stdout, &format!("pointer step {step}: frames presented"));
            assert!(frames >= 1, "step {step} drew nothing:\n{stdout}");
        }
        let frames = number_after(&stdout, "pointer: frames presented");
        assert!(frames >= 2, "{frames} frames for 2 steps:\n{stdout}");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_start_that_fills_a_warm_shader_cache_takes_the_steps_too() {
        // Its steps, like a measured run's: with a stand-in that draws
        // nothing for them, the run fails in the filling start, before run 1.
        let output = measure(
            &[
                "--shader-cache",
                "warm",
                "--first-frame-timeout",
                "5",
                "--place-pointer",
                "640,360",
                "--move-pointer",
                "600,300",
            ],
            STILL,
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains("filling the shader cache"), "{stderr}");
        assert!(
            stderr.contains("presented no frame for pointer step 1 of 1"),
            "{stderr}"
        );
        assert!(!stderr.contains("measure: run 1"), "{stderr}");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn each_step_is_measured_after_each_show() {
        let dir = tempfile::tempdir().unwrap();
        let stand_in = summoned(dir.path(), true);
        let (summon, dismiss, script) = (stand_in.summon, stand_in.dismiss, stand_in.script);
        let output = measure(
            &[
                "--summon",
                &summon,
                "--dismiss",
                &dismiss,
                "--shows",
                "2",
                "--place-pointer",
                "640,360",
                "--move-pointer",
                "600,300",
            ],
            &script,
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        for show in 1..=2 {
            let row = format!(
                "show {show}: pointer step 1 of 1 (to 1:600,300): CPU from the move to settled"
            );
            assert!(stdout.contains(&row), "no {row:?} in:\n{stdout}");
            let frames = number_after(
                &stdout,
                &format!("show {show}: pointer step 1 of 1 (to 1:600,300): frames presented"),
            );
            assert!(frames >= 1, "show {show}:\n{stdout}");
        }
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_step_that_leads_to_no_frame_fails_the_run() {
        let output = measure(
            &[
                "--first-frame-timeout",
                "5",
                "--place-pointer",
                "640,360",
                "--move-pointer",
                "600,300",
            ],
            STILL,
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("presented no frame for pointer step 1 of 1 (to 1:600,300)"),
            "{stderr}"
        );
        assert!(output.stdout.is_empty(), "a report was printed");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_client_refusing_a_point_fails_the_run() {
        // Off the 1280x720 output: the client refuses it and ends.
        let output = measure(
            &["--place-pointer", "640,360", "--move-pointer", "5000,10"],
            DRAWS,
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("the input client (cargo xtask input) did not take `point "),
            "{stderr}"
        );
        assert!(stderr.contains("is not on"), "{stderr}");
        assert!(output.stdout.is_empty(), "a report was printed");
    }
}

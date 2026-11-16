//! `cargo xtask session --output-size`: outputs of the sizes asked for, as
//! a client of the session sees them, with and without a scale, and the
//! refusals.
//!
//! The session's command is this test binary again, running only
//! [`sizes::report`] (ignored, so a plain run passes over it), which
//! connects to the session as the test Wayland client and prints each
//! output's mode and logical size. The tests that need a compositor are
//! ignored by a plain `cargo test`; they start only isolated sessions of
//! their own. Name the compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=labwc cargo test -p xtask --test session_sizes -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod sizes {
    use std::process::{Command, Output, Stdio};

    use deskrinch_test_client::TestClient;

    /// The variable that makes [`report`] report.
    const REPORT: &str = "DESKRINCH_SIZES_REPORT";

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// `xtask session` with `options` and the compositor, its command
    /// [`report`].
    fn session_reporting(options: &[&str]) -> Output {
        let me = std::env::current_exe().unwrap();
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("session")
            .args(options)
            .arg(compositor())
            .arg(me)
            .args(["--exact", "sizes::report", "--ignored", "--nocapture"])
            .env(REPORT, "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// Each output's mode and logical size, in the order the session made
    /// them, as [`report`] printed them (the test harness writes its own
    /// `test … ` before the first, on the same line).
    fn reported(output: &Output) -> Vec<((i32, i32), (i32, i32))> {
        let said = String::from_utf8_lossy(&output.stdout);
        said.lines()
            .filter_map(|line| line.split_once("output ").map(|(_, rest)| rest))
            .map(|line| {
                let numbers: Vec<i32> = line
                    .split_whitespace()
                    .skip(1)
                    .filter_map(|part| part.parse().ok())
                    .collect();
                assert_eq!(numbers.len(), 4, "{line}");
                ((numbers[0], numbers[1]), (numbers[2], numbers[3]))
            })
            .collect()
    }

    /// Run inside the session by the tests: one line per output,
    /// `output NAME W H LW LH`, its mode and its logical size.
    #[test]
    #[ignore = "run by this file's tests, inside a session"]
    fn report() {
        if std::env::var_os(REPORT).is_none() {
            return;
        }
        let client = TestClient::connect().unwrap();
        for name in client.output_names() {
            let info = client.output_info(&name).unwrap();
            let (mode, logical) = (info.mode.unwrap(), info.logical_size.unwrap());
            println!(
                "output {name} {} {} {} {}",
                mode.0, mode.1, logical.0, logical.1
            );
        }
    }

    /// Two outputs asked for 1920x1080 and 3840x2160 have those modes, at
    /// their logical size, as a client of the session sees them; the
    /// session says so.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_outputs_have_the_sizes_asked_for() {
        let output = session_reporting(&[
            "--outputs",
            "2",
            "--output-size",
            "1920x1080",
            "--output-size",
            "3840x2160",
        ]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            reported(&output),
            [((1920, 1080), (1920, 1080)), ((3840, 2160), (3840, 2160))],
            "{output:?}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("2 output(s) of 1920x1080, 3840x2160 at scale 1"),
            "{stderr}"
        );
    }

    /// Without the option, every output is 1280x720, as before.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn without_it_the_outputs_are_the_default_size() {
        let output = session_reporting(&["--outputs", "2"]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            reported(&output),
            [((1280, 720), (1280, 720)), ((1280, 720), (1280, 720))],
            "{output:?}"
        );
    }

    /// With a scale, the mode is the size asked for and the logical size
    /// is the mode over the scale; an output given no size is the default.
    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn with_a_scale_the_logical_size_follows() {
        let output = session_reporting(&[
            "--outputs",
            "2",
            "--scale",
            "2",
            "--output-size",
            "3840x2160",
        ]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            reported(&output),
            [((3840, 2160), (1920, 1080)), ((1280, 720), (640, 360))],
            "{output:?}"
        );
    }

    /// `xtask session` with `options`, refused before any session starts.
    fn refused(options: &[&str]) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("session")
            .args(options)
            .args(["sway", "true"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{options:?}: {output:?}");
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    #[test]
    fn refuses_a_size_with_no_area_or_that_is_not_one() {
        let zero = refused(&["--output-size", "0x1080"]);
        assert!(zero.contains("0x1080 has no area"), "{zero}");
        let malformed = refused(&["--output-size", "1920"]);
        assert!(malformed.contains("\"1920\" is not a size"), "{malformed}");
    }

    #[test]
    fn refuses_more_sizes_than_outputs() {
        let said = refused(&[
            "--outputs",
            "1",
            "--output-size",
            "1920x1080",
            "--output-size",
            "3840x2160",
        ]);
        assert!(
            said.contains("2 output size(s) asked for 1 output(s)"),
            "{said}"
        );
    }
}

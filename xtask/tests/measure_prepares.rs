//! `cargo xtask measure --summon --prepares` end to end, on stand-ins for a
//! summoned rinch program in isolated sessions: the first settle waits for
//! a program's first preparation, a program that never prepares fails the
//! run, and without the flag a run whose show 1 came before any preparation
//! says so.
//!
//! Ignored by a plain `cargo test`; name the compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure_prepares -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod report;
#[cfg(test)]
mod stand_in;

#[cfg(test)]
mod measure_prepares {
    use std::path::Path;
    use std::process::{Command, Output, Stdio};

    use crate::report::number_after;
    use crate::stand_in::summoned;

    /// What a run's notes say when show 1 came before any preparation.
    const NOTE: &str = "summoned before the program prepared anything";

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// `xtask measure --summon` of the stand-in in `dir`, which first runs
    /// `start`, then draws when summoned; one show, and `options` besides.
    fn measure(dir: &Path, start: &str, options: &[&str]) -> Output {
        let stand_in = summoned(dir, false);
        let (summon, dismiss) = (stand_in.summon, stand_in.dismiss);
        let script = format!("{start}{}", stand_in.script);
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .args(["--summon", &summon, "--dismiss", &dismiss, "--shows", "1"])
            .args(options)
            .args(["sh", "-c", &script])
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_program_that_prepares_late_is_waited_for() {
        // Quiet for three seconds, then its first preparation: a settle of
        // one quiet second alone would summon it before.
        let dir = tempfile::tempdir().unwrap();
        let late = "sleep 3; echo '[PERF] prepared 1: a' >&2; ";
        let output = measure(dir.path(), late, &["--prepares"]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        let before = number_after(&stdout, "show 1: prepared frames before it, in all");
        assert_eq!(before, 1, "show 1 came before the preparation:\n{stdout}");
        assert!(!stdout.contains(NOTE), "{stdout}");
        assert!(
            stdout.contains(
                "Prepares (--prepares): the first show waited for the program's first preparation"
            ),
            "the header does not say the run waited:\n{stdout}"
        );
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_program_that_never_prepares_fails_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let output = measure(
            dir.path(),
            "",
            &["--prepares", "--first-frame-timeout", "4"],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains("prepared nothing within 4 s"), "{stderr}");
        assert!(output.stdout.is_empty(), "a report was printed");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn without_the_flag_a_show_before_any_preparation_is_noted() {
        let dir = tempfile::tempdir().unwrap();
        let output = measure(dir.path(), "", &[]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        let before = number_after(&stdout, "show 1: prepared frames before it, in all");
        assert_eq!(before, 0, "{stdout}");
        assert!(stdout.contains(NOTE), "no note in:\n{stdout}");
        assert!(stdout.contains("--prepares waits"), "{stdout}");
        assert!(!stdout.contains("Prepares (--prepares)"), "{stdout}");
    }
}

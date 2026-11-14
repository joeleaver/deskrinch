//! `cargo xtask measure --instructions` end to end, on a stand-in for a
//! rinch program under Valgrind's Callgrind, in an isolated session: the
//! settle comes before the idle window in this mode too, and its count is
//! noted.
//!
//! Apart from `tests/measure.rs` because it needs Valgrind, which CI does
//! not install. Ignored by a plain `cargo test`; name the compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure_callgrind -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod measure_callgrind {
    use std::process::{Command, Stdio};

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// The number after `label` in `report`, its digits' separators removed.
    fn number_after(report: &str, label: &str) -> u64 {
        let rest =
            &report[report.find(label).unwrap_or_else(|| panic!("no {label:?}")) + label.len()..];
        rest.trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == ',')
            .filter(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or_else(|_| panic!("no number after {label:?} in:\n{report}"))
    }

    #[test]
    #[ignore = "needs Valgrind and a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_settle_is_kept_out_of_the_idle_window_and_noted() {
        // After its first frame the stand-in works (a counting loop), then
        // sleeps: work after the first frame, as an index built on a thread
        // of its own is. The settle is long enough, even under Callgrind,
        // for the work to fall inside it. The shell itself stays (no `exec`):
        // Callgrind follows no `exec`, and a program that left it would
        // leave nothing to dump.
        let script = "echo '[PERF] frame 1: style 0.10ms' >&2; \
                      i=0; while [ $i -lt 20000 ]; do i=$((i+1)); done; \
                      while :; do sleep 1; done";
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .args(["--instructions", "--settle", "20", "--idle", "2"])
            .args(["sh", "-c", script])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        let settle = number_after(
            &stdout,
            "instructions during the settle, kept out of the idle window:",
        );
        let idle = number_after(&stdout, "instructions in the idle window (Ir)");
        assert!(
            settle > idle,
            "the work after the first frame fell in the settle ({settle} Ir), not the idle \
             window ({idle} Ir):\n{stdout}"
        );
        assert!(stdout.contains(" Ir in 20 s"), "{stdout}");
    }
}

//! `cargo xtask measure --instructions` end to end, on a stand-in for a
//! rinch program under Valgrind's Callgrind, in an isolated session: the
//! settle comes before the idle window in this mode too, and its count is
//! noted; with `--summon`, each show and each hide is read from its own
//! dump.
//!
//! Apart from `tests/measure.rs` because it needs Valgrind (CI installs it
//! for the compositor jobs). Ignored by a plain `cargo test`; name the
//! compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure_callgrind -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod report;

#[cfg(test)]
mod measure_callgrind {
    use std::fs;
    use std::process::{Command, Stdio};

    use crate::report::number_after;

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
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

    #[test]
    #[ignore = "needs Valgrind and a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn each_show_and_each_hide_reads_its_own_dump() {
        // The stand-in starts hidden and prepares once. A summon (a file it
        // watches) makes it count to 2,000 and present a frame; a dismiss
        // makes it count to 60,000 and prepare a frame. So each hide does
        // far more work than each show: a row that read the other's dump
        // would turn the comparison round. The shell stays, for Callgrind
        // to dump.
        let dir = tempfile::tempdir().unwrap();
        let (show, hide) = (dir.path().join("show"), dir.path().join("hide"));
        let summon = dir.path().join("summon.sh");
        fs::write(&summon, format!("touch {}\n", show.display())).unwrap();
        let dismiss = dir.path().join("dismiss.sh");
        fs::write(&dismiss, format!("touch {}\n", hide.display())).unwrap();
        let script = format!(
            "echo '[PERF] prepared 1: a' >&2; n=0; p=1; while :; do \
             if [ -e {show} ]; then rm {show}; \
             i=0; while [ $i -lt 2000 ]; do i=$((i+1)); done; \
             n=$((n + 1)); echo \"[PERF] frame $n: a\" >&2; fi; \
             if [ -e {hide} ]; then rm {hide}; \
             i=0; while [ $i -lt 60000 ]; do i=$((i+1)); done; \
             p=$((p + 1)); echo \"[PERF] prepared $p: a\" >&2; fi; \
             sleep 0.05; done",
            show = show.display(),
            hide = hide.display()
        );
        let summon = summon.display().to_string();
        let dismiss = dismiss.display().to_string();
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .args(["--instructions", "--settle", "1", "--idle", "1"])
            .args(["--summon", &summon, "--dismiss", &dismiss, "--shows", "2"])
            .args(["sh", "-c", &script])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        for n in 1..=2 {
            let show = number_after(
                &stdout,
                &format!("show {n}: instructions from the summon to its first frame (Ir)"),
            );
            let hide = number_after(
                &stdout,
                &format!("hide {n}: instructions from the dismiss to settled (Ir)"),
            );
            assert!(
                show * 5 < hide,
                "show {n} ({show} Ir) is not well below hide {n} ({hide} Ir):\n{stdout}"
            );
            let prepared = number_after(
                &stdout,
                &format!("hide {n}: prepared frames from the dismiss to settled"),
            );
            assert_eq!(prepared, 1, "hide {n}:\n{stdout}");
            let before = number_after(
                &stdout,
                &format!("show {n}: prepared frames before it, in all"),
            );
            assert_eq!(before, n, "show {n}:\n{stdout}");
        }
    }

    #[test]
    #[ignore = "needs Valgrind and a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn several_windows_are_counted_under_callgrind_too() {
        // Two windows, each presenting a first frame and then a frame a
        // second. As above, the shell stays, for Callgrind to dump.
        let script = "echo '[PERF] window 1 opened: title=\"stand-in\" output=\"HEADLESS-1\"' >&2; \
                      echo '[PERF] window 2 opened: title=\"stand-in\" output=\"HEADLESS-2\"' >&2; \
                      echo '[PERF] window 1 frame 1: style 0.10ms' >&2; \
                      echo '[PERF] window 2 frame 1: style 0.10ms' >&2; \
                      n=1; while :; do sleep 1; n=$((n+1)); \
                      echo \"[PERF] window 1 frame $n: a\" >&2; \
                      echo \"[PERF] window 2 frame $n: a\" >&2; done";
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .args(["--instructions", "--settle", "1", "--idle", "3"])
            .args(["--windows", "2"])
            .args(["sh", "-c", script])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        for row in [
            "instructions to first frame (Ir)",
            "frames in the idle window",
            "  of it \"stand-in\" on HEADLESS-1",
            "  of it \"stand-in\" on HEADLESS-2",
        ] {
            assert!(stdout.contains(row), "no {row:?} in:\n{stdout}");
        }
        assert!(
            !stdout.contains("process CPU when"),
            "CPU rows under Callgrind:\n{stdout}"
        );
    }
}

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
    use std::path::{Path, PathBuf};
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
        // The stand-in starts hidden and prepares once. A summon makes it
        // count to 2,000 and present a frame; a dismiss makes it count to
        // 60,000 and prepare a frame. So each hide does far more work than
        // each show: a row that read the other's dump would turn the
        // comparison round. Summon and dismiss write a word into a FIFO the
        // stand-in reads, so that idle it waits without running: a polling
        // loop would fork, which under Callgrind costs more than a settled
        // program may. The shell stays, for Callgrind to dump.
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("asked");
        let made = Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(made.success(), "mkfifo: {made}");
        let summon = dir.path().join("summon.sh");
        fs::write(&summon, format!("echo show > {}\n", fifo.display())).unwrap();
        let dismiss = dir.path().join("dismiss.sh");
        fs::write(&dismiss, format!("echo hide > {}\n", fifo.display())).unwrap();
        // It starts quiet, sleeping three seconds before its first
        // preparation, as a stand-in starved of CPU on a loaded machine
        // does. A sleep and not a count: only an idle start passes a quiet
        // window (a count runs, and keeps the settle waiting), and two
        // windows of idleness against a wait of one summon it before it
        // prepared every time, not by luck, unless `--prepares` makes the
        // first show wait for the preparation.
        let script = format!(
            "sleep 3; echo '[PERF] prepared 1: a' >&2; n=0; p=1; \
             while read asked < {fifo}; do \
             if [ \"$asked\" = show ]; then \
             i=0; while [ $i -lt 2000 ]; do i=$((i+1)); done; \
             n=$((n + 1)); echo \"[PERF] frame $n: a\" >&2; fi; \
             if [ \"$asked\" = hide ]; then \
             i=0; while [ $i -lt 60000 ]; do i=$((i+1)); done; \
             p=$((p + 1)); echo \"[PERF] prepared $p: a\" >&2; fi; \
             done",
            fifo = fifo.display()
        );
        let summon = summon.display().to_string();
        let dismiss = dismiss.display().to_string();
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .args(["--instructions", "--settle", "1", "--idle", "1"])
            .args(["--summon", &summon, "--dismiss", &dismiss, "--shows", "2"])
            .arg("--prepares")
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

    /// A stand-in, run by `bash`, that presents a frame every half second
    /// once shown (from the start when `shown` is 1), and shows and hides
    /// when `fifo` is written `s` and `h`. It waits with `read -t` on the
    /// FIFO, opened for reading and writing so that its opening never
    /// blocks: a builtin, so that it forks nothing under Callgrind. Each
    /// word is one byte (`read -n 1`): `read` takes a pipe a byte at a time,
    /// and a longer word could be cut by the timeout, slow under Callgrind.
    fn every_half_second(fifo: &Path, shown: u8) -> String {
        format!(
            "exec 3<>{fifo}; n=0; shown={shown}; \
             if [ $shown = 1 ]; then n=1; echo '[PERF] frame 1: a' >&2; fi; \
             while :; do \
             if read -t 0.5 -n 1 -u 3 asked; then \
             if [ \"$asked\" = s ]; then shown=1; n=$((n + 1)); \
             echo \"[PERF] frame $n: a\" >&2; fi; \
             if [ \"$asked\" = h ]; then shown=0; fi; \
             elif [ $shown = 1 ]; then n=$((n + 1)); echo \"[PERF] frame $n: a\" >&2; fi; \
             done",
            fifo = fifo.display()
        )
    }

    /// A FIFO in `dir`, for [`every_half_second`].
    fn fifo(dir: &Path) -> PathBuf {
        let fifo = dir.join("asked");
        let made = Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(made.success(), "mkfifo: {made}");
        fifo
    }

    #[test]
    #[ignore = "needs Valgrind and a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn each_pointer_step_reads_its_own_dump_after_the_idle_window() {
        // Each instruction row must be the total of the dump Callgrind wrote
        // for its span, which names the span in its description (its hint):
        // checked against the profile, kept with --profiles. Counts alone
        // cannot tell: the stand-in's spans are whole numbers of loop turns,
        // and two of a turn's length count within a few instructions.
        let dir = tempfile::tempdir().unwrap();
        let fifo = fifo(dir.path());
        let profiles = dir.path().join("profiles");
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .args(["--instructions", "--settle", "1", "--idle", "6"])
            .arg("--profiles")
            .arg(&profiles)
            .args(["--place-pointer", "640,360"])
            .args(["--move-pointer", "600,300", "--move-pointer", "640,360"])
            .args(["bash", "-c", &every_half_second(&fifo, 1)])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        let dumps = named_dumps(&profiles.join("run-1"));
        for (row, hint) in [
            ("instructions to first frame (Ir)", "first-frame"),
            ("instructions in the idle window (Ir)", "idle"),
            (
                "pointer step 1 of 2 (to 1:600,300): instructions from the move to settled (Ir)",
                "pointer-1",
            ),
            (
                "pointer step 2 of 2 (to 1:640,360): instructions from the move to settled (Ir)",
                "pointer-2",
            ),
        ] {
            assert_eq!(
                Some(number_after(&stdout, row)),
                dumps.get(hint).copied(),
                "{row:?} is not the dump {hint:?}'s total; the dumps: {dumps:?}\n{stdout}"
            );
        }
        // Start-up and settle together: the two dumps' totals summed.
        assert_eq!(
            Some(number_after(
                &stdout,
                "instructions to the end of the settle (Ir)"
            )),
            dumps
                .get("first-frame")
                .zip(dumps.get("settle"))
                .map(|(first, settle)| first + settle),
            "the end of the settle is not the first frame's dump and the settle's; the dumps: \
             {dumps:?}\n{stdout}"
        );
        for step in ["1 of 2 (to 1:600,300)", "2 of 2 (to 1:640,360)"] {
            let frames = number_after(&stdout, &format!("pointer step {step}: frames presented"));
            assert!(frames >= 1, "{stdout}");
        }
    }

    /// Each dump of the profile in `dir` (`callgrind.out.N`), by the hint it
    /// was written with (`desc: Trigger: dump HINT`): its total.
    fn named_dumps(dir: &Path) -> std::collections::BTreeMap<String, u64> {
        fs::read_dir(dir)
            .unwrap()
            .map(|entry| fs::read_to_string(entry.unwrap().path()).unwrap())
            .filter_map(|text| {
                let hint = text
                    .lines()
                    .find_map(|line| line.strip_prefix("desc: Trigger: dump "))?
                    .trim_end()
                    .to_owned();
                let total = text
                    .lines()
                    .find_map(|line| line.strip_prefix("totals: "))?
                    .trim()
                    .parse()
                    .ok()?;
                Some((hint, total))
            })
            .collect()
    }

    #[test]
    #[ignore = "needs Valgrind and a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn each_pointer_step_reads_its_own_dump_after_each_show() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = fifo(dir.path());
        let summon = dir.path().join("summon.sh");
        fs::write(&summon, format!("printf s > {}\n", fifo.display())).unwrap();
        let dismiss = dir.path().join("dismiss.sh");
        fs::write(&dismiss, format!("printf h > {}\n", fifo.display())).unwrap();
        let summon = summon.display().to_string();
        let dismiss = dismiss.display().to_string();
        let profiles = dir.path().join("profiles");
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .args(["--instructions", "--settle", "1", "--idle", "1"])
            .arg("--profiles")
            .arg(&profiles)
            .args(["--summon", &summon, "--dismiss", &dismiss, "--shows", "2"])
            .args(["--place-pointer", "640,360", "--move-pointer", "600,300"])
            .args(["bash", "-c", &every_half_second(&fifo, 0)])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        let dumps = named_dumps(&profiles.join("run-1"));
        for show in 1..=2 {
            let step = format!("show {show}: pointer step 1 of 1 (to 1:600,300)");
            for (row, hint) in [
                (
                    format!("show {show}: instructions from the summon to its first frame (Ir)"),
                    format!("show-{show}"),
                ),
                (
                    format!("{step}: instructions from the move to settled (Ir)"),
                    format!("show-{show}-pointer-1"),
                ),
                (
                    format!("hide {show}: instructions from the dismiss to settled (Ir)"),
                    format!("hide-{show}"),
                ),
            ] {
                assert_eq!(
                    Some(number_after(&stdout, &row)),
                    dumps.get(&hint).copied(),
                    "{row:?} is not the dump {hint:?}'s total; the dumps: {dumps:?}\n{stdout}"
                );
            }
            let frames = number_after(&stdout, &format!("{step}: frames presented"));
            assert!(frames >= 1, "{stdout}");
        }
    }
}

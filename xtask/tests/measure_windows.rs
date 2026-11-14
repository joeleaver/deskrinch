//! `cargo xtask measure --windows N` end to end, on stand-ins for a rinch
//! program with several windows: shell scripts that print rinch's
//! per-window `[PERF]` lines (`window <id> opened: …`, `window <id> frame
//! N: …`, `window <id> closed`) in the isolated sessions the task starts.
//! Beside them, the report of a one-window program measured without
//! `--windows`, whose rows are held to a list captured before `--windows`
//! existed (`tests/fixtures/one-window-rows.txt`). Nothing here asserts on
//! time.
//!
//! Ignored by a plain `cargo test`, because they need a compositor
//! installed; they start only isolated sessions of their own. Name the
//! compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure_windows -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod measure_windows {
    use std::fmt::Write as _;
    use std::process::{Command, Output, Stdio};

    /// The rows of a one-window report, as captured (see its first lines).
    const ONE_WINDOW_ROWS: &str = include_str!("fixtures/one-window-rows.txt");

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

    /// The row names of a report's tables, in order. A row whose name says
    /// what the machine has (a thread's name, a mapping, a GPU memory
    /// region) is written with `*` for that part, and a run of such rows
    /// as one, so that the list says which rows there are, not what `sh`
    /// maps on this machine.
    fn rows(report: &str) -> Vec<String> {
        let mut rows: Vec<String> = Vec::new();
        let mut width = None;
        for line in report.lines() {
            if line.starts_with("  metric") {
                // The name column ends where the median's right-aligned
                // header begins: two spaces, then 14 wide.
                width = line.find("median").map(|at| at + "median".len() - 18);
                continue;
            }
            let Some(name_width) = width else { continue };
            if line.is_empty() {
                width = None;
                continue;
            }
            let name = line.get(2..2 + name_width).unwrap_or(line).trim_end();
            let row = machine_free(name);
            if rows.last() != Some(&row) {
                rows.push(row);
            }
        }
        rows
    }

    /// `name` with what the machine decides written as `*`, the rows that
    /// come in pairs (a thread group's time and its count, a mapping's Rss
    /// and Pss) as one.
    fn machine_free(name: &str) -> String {
        let bare = name.trim_start();
        let groups = [
            (
                &["thread ", "threads named "][..],
                "  thread *, threads named *",
            ),
            (&["Rss: ", "Pss: "][..], "  Rss: *, Pss: *"),
            (&["GPU memory "][..], "GPU memory *"),
        ];
        for (prefixes, row) in groups {
            if prefixes.iter().any(|prefix| bare.starts_with(prefix)) {
                return row.to_owned();
            }
        }
        name.to_owned()
    }

    /// The fixture's rows: its lines but the comments.
    fn fixture_rows() -> Vec<String> {
        ONE_WINDOW_ROWS
            .lines()
            .filter(|line| !line.starts_with('#'))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_one_window_report_has_the_rows_it_had_before_windows_were_counted() {
        let script = "echo '[PERF] frame 1: style 0.10ms' >&2; \
                      while :; do sleep 0.2; echo '[PERF] frame 2: paint_frames=1' >&2; done";
        let output = measure(&[], script);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert_eq!(rows(&stdout), fixture_rows(), "in:\n{stdout}");
    }

    /// The `opened` line of window `id`, on `output`.
    fn opened(id: u32, output: &str) -> String {
        format!(
            "echo '[PERF] window {id} opened: title=\"stand-in\" app_id=\"org.deskrinch.StandIn\" \
             output=\"{output}\" output_id={id}' >&2; "
        )
    }

    /// Script lines that present a first frame of each of `ids`, then go on
    /// presenting frames of each, five a second, for as long as it runs.
    fn presents(ids: &[u32]) -> String {
        let mut first = String::new();
        let mut more = String::new();
        for id in ids {
            let _ = write!(
                first,
                "echo '[PERF] window {id} frame 1: style 0.10ms' >&2; "
            );
            let _ = write!(
                more,
                "echo \"[PERF] window {id} frame $n: paint_frames=1\" >&2; "
            );
        }
        format!("{first}n=1; while :; do sleep 0.2; n=$((n + 1)); {more}done")
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn two_windows_are_each_named_and_counted() {
        let script = format!(
            "{}{}{}",
            opened(1, "HEADLESS-1"),
            opened(2, "HEADLESS-2"),
            presents(&[1, 2])
        );
        let output = measure(&["--windows", "2"], &script);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        for row in [
            "CPU to first frame, user+system",
            "process CPU when \"stand-in\" on HEADLESS-1 first presented",
            "process CPU when \"stand-in\" on HEADLESS-2 first presented",
            "(cumulative: the process's CPU by then, work for every window included, not \
             what that window cost)",
            "frames in the idle window",
            "  of it \"stand-in\" on HEADLESS-1",
            "  of it \"stand-in\" on HEADLESS-2",
            "frames in all",
            "wall time when \"stand-in\" on HEADLESS-2 first presented",
            "windows: 2 expected",
        ] {
            assert!(stdout.contains(row), "no {row:?} in:\n{stdout}");
        }
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn fewer_windows_than_expected_fail_without_a_table() {
        let script = format!(
            "{}{}echo 'only one output here' >&2; {}",
            opened(1, "HEADLESS-1"),
            opened(2, "HEADLESS-2"),
            presents(&[1])
        );
        let output = measure(&["--windows", "2", "--first-frame-timeout", "3"], &script);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
        for said in [
            "1 of the 2 windows expected presented a first frame within 3 s",
            "\"stand-in\" on HEADLESS-1: presented",
            "\"stand-in\" on HEADLESS-2: opened, no frame presented",
            "only one output here",
        ] {
            assert!(stderr.contains(said), "no {said:?} in:\n{stderr}");
        }
        assert!(!stdout.contains("frames in all"), "a table:\n{stdout}");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn more_windows_than_expected_fail() {
        let script = format!(
            "{}{}{}{}",
            opened(1, "HEADLESS-1"),
            opened(2, "HEADLESS-2"),
            opened(3, "HEADLESS-3"),
            presents(&[1, 2, 3])
        );
        let output = measure(&["--windows", "2"], &script);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
        assert!(
            stderr.contains("3 windows open where 2 were expected"),
            "{stderr}"
        );
        assert!(!stdout.contains("frames in all"), "a table:\n{stdout}");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_window_replaced_during_the_run_fails_it_as_outside_what_is_measured() {
        // Window 2 closes a second after the first frames and window 3 takes
        // its place on the same output, as when an output is turned off and
        // on again: the count stays 2, the windows do not.
        let script = format!(
            "{}{}echo '[PERF] window 1 frame 1: style 0.10ms' >&2; \
             echo '[PERF] window 2 frame 1: style 0.10ms' >&2; sleep 1; \
             echo '[PERF] window 2 closed' >&2; {}\
             echo '[PERF] window 3 frame 1: style 0.10ms' >&2; exec sleep 600",
            opened(1, "HEADLESS-1"),
            opened(2, "HEADLESS-2"),
            opened(3, "HEADLESS-2"),
        );
        let output = measure(&["--windows", "2"], &script);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
        for said in [
            "its windows changed after start-up",
            "closed: \"stand-in\" on HEADLESS-2",
            "opened: \"stand-in\" on HEADLESS-2",
            "outside what measure measures",
        ] {
            assert!(stderr.contains(said), "no {said:?} in:\n{stderr}");
        }
        assert!(!stdout.contains("frames in all"), "a table:\n{stdout}");
    }
}

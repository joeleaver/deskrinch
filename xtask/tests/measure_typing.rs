//! `cargo xtask measure --type` end to end, on a stand-in for a rinch
//! program in an isolated session, typed into by the real input client
//! (`cargo xtask input`). The stand-in hears no keys: it says it has the keyboard
//! when shown, presents a frame every 0.3 s while shown (so every key is
//! followed by one), and at the hide says its query had a length the test
//! chooses. What is checked is the orchestration: each key measured, the
//! frames, and each way a typed run fails.
//!
//! Ignored by a plain `cargo test`; name the compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure_typing -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod fifo;
#[cfg(test)]
mod report;

#[cfg(test)]
mod measure_typing {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Output, Stdio};

    use crate::fifo::fifo;
    use crate::report::number_after;

    const KEYBOARD: &str = "the stand-in has the keyboard";
    const QUERY: &str = "hidden with a query of {} characters";

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// How the stand-in behaves.
    struct StandIn {
        /// Whether it says it has the keyboard when shown.
        says_keyboard: bool,
        /// Whether it presents frames while shown, after the first.
        draws: bool,
        /// The query length it gives at the hide.
        length: usize,
    }

    /// `xtask measure --type text` of the stand-in, `query_line` as the
    /// pattern given, with `shows` shows and `options` besides.
    fn measure(
        dir: &Path,
        stand_in: &StandIn,
        text: &str,
        query_line: &str,
        options: &[&str],
    ) -> Output {
        // It waits blocked on a FIFO, as an idle program does (no polling):
        // the summon writes `s`, the dismiss `h`; while shown and drawing it
        // presents a frame every 0.3 s (`read -t`).
        let asked = fifo(dir, "asked").display().to_string();
        let summon = dir.join("summon.sh");
        fs::write(&summon, format!("printf 's\\n' > {asked}\n")).unwrap();
        let dismiss = dir.join("dismiss.sh");
        fs::write(&dismiss, format!("printf 'h\\n' > {asked}\n")).unwrap();
        let keyboard = if stand_in.says_keyboard {
            format!("echo '{KEYBOARD}' >&2; ")
        } else {
            String::new()
        };
        let draws = u8::from(stand_in.draws);
        let said = QUERY.replace("{}", &stand_in.length.to_string());
        let program = dir.join("stand-in.bash");
        fs::write(
            &program,
            format!(
                "exec 3<>{asked}; n=0; shown=0\n\
                 while :; do\n\
                 if [ $shown = 1 ] && [ {draws} = 1 ]; then\n\
                 read -t 0.3 -u 3 asked || asked=tick\n\
                 else\n\
                 read -u 3 asked\n\
                 fi\n\
                 case $asked in\n\
                 s) shown=1; n=$((n + 1)); echo \"[PERF] frame $n: paint_frames=1\" >&2; \
                 {keyboard};;\n\
                 h) shown=0; echo '{said}' >&2 ;;\n\
                 tick) n=$((n + 1)); echo \"[PERF] frame $n: paint_frames=1\" >&2 ;;\n\
                 esac\n\
                 done\n"
            ),
        )
        .unwrap();
        let script = format!("exec bash {}", program.display());
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .args(["--summon", &summon.display().to_string()])
            .args(["--dismiss", &dismiss.display().to_string()])
            .args(["--type", text, "--keyboard-line", KEYBOARD])
            .args(["--query-line", query_line])
            .args(options)
            .args(["sh", "-c", &script])
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    const TAKES_ALL: StandIn = StandIn {
        says_keyboard: true,
        draws: true,
        length: 3,
    };

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn each_key_is_measured_to_settled_and_followed_by_a_frame() {
        let dir = tempfile::tempdir().unwrap();
        let output = measure(dir.path(), &TAKES_ALL, "abc", QUERY, &["--shows", "2"]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        for show in 1..=2 {
            for key in 1..=3 {
                let row = format!("show {show}: key {key} of 3: CPU from the key to settled");
                assert!(stdout.contains(&row), "no {row:?} in:\n{stdout}");
            }
            let total = format!(
                "show {show}: typing 3 characters, the keys' sum: CPU from the key to settled"
            );
            assert!(stdout.contains(&total), "no {total:?} in:\n{stdout}");
            for key in 1..=3 {
                let row = format!("show {show}: key {key} of 3: frames presented");
                assert!(number_after(&stdout, &row) >= 1, "{row}:\n{stdout}");
                // The keys have the two rows the pointer's steps have:
                // this stand-in's frames are paints alone.
                let row = format!("show {show}: key {key} of 3: of them with reactive work");
                assert_eq!(number_after(&stdout, &row), 0, "{row}:\n{stdout}");
            }
            let frames = number_after(&stdout, &format!("show {show}: typing: frames presented"));
            assert!(
                frames >= 3,
                "show {show}: {frames} frames for 3 keys:\n{stdout}"
            );
        }
        assert!(
            stdout.contains("Typed after each show: \"abc\""),
            "{stdout}"
        );
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_query_of_another_length_fails_the_run_with_both_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let short = StandIn {
            length: 2,
            ..TAKES_ALL
        };
        let output = measure(dir.path(), &short, "abc", QUERY, &[]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("its query had 2 characters, but 3 were typed"),
            "{stderr}"
        );
        assert!(output.stdout.is_empty(), "a report was printed");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_program_that_never_says_it_has_the_keyboard_fails_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let silent = StandIn {
            says_keyboard: false,
            ..TAKES_ALL
        };
        let output = measure(
            dir.path(),
            &silent,
            "abc",
            QUERY,
            &["--first-frame-timeout", "5"],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains(&format!(
                "never wrote a line with {KEYBOARD:?} after show 1"
            )),
            "{stderr}"
        );
        assert!(output.stdout.is_empty(), "a report was printed");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_key_that_leads_to_no_frame_fails_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let still = StandIn {
            draws: false,
            ..TAKES_ALL
        };
        let output = measure(
            dir.path(),
            &still,
            "abc",
            QUERY,
            &["--first-frame-timeout", "5"],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("presented no frame for key 1 of 3 ('a') in show 1"),
            "{stderr}"
        );
        assert!(output.stdout.is_empty(), "a report was printed");
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_query_line_the_program_does_not_write_fails_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let output = measure(
            dir.path(),
            &TAKES_ALL,
            "abc",
            "the query is {} long",
            &["--first-frame-timeout", "5"],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("wrote no line like \"the query is {} long\" at hide 1"),
            "{stderr}"
        );
        assert!(output.stdout.is_empty(), "a report was printed");
    }
}

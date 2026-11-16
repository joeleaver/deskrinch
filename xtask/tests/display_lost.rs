//! What each shell component does when its display goes away: the
//! compositor it is a client of is killed while it runs, and it ends with
//! deskrinch-shell's `DISPLAY_LOST` status, having said
//! `DISPLAY_LOST_LINE`.
//!
//! Each test runs the component in a session of its own, `cargo xtask
//! session` of the run's compositor, nested in the one `cargo xtask
//! compositor-tests` runs this binary in: ending that session's compositor
//! must not end the suite's. The component's stderr goes to a file; the
//! test waits, in Rust, for its first frame there (`RINCH_PERF`): presented,
//! or prepared for the launcher started hidden. It then lets the session's
//! command kill the compositor (`DESKRINCH_SESSION_COMPOSITOR_PID`), wait
//! for the component and write its status. `xtask session` waits for its command whatever happens to
//! the compositor, so the status and the last lines come out before the
//! session ends.

#[cfg(test)]
mod lost {
    use std::fs;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};

    use deskrinch_shell::exit_status::{DISPLAY_LOST, DISPLAY_LOST_LINE};
    use deskrinch_test_component::perf::{PerfLine, parse};
    use deskrinch_test_component::programs::program;
    use deskrinch_test_wait::{Deadline, PATIENCE, eventually};

    /// The session's command: the component (`$0`, its arguments after) in
    /// the background with its stderr in `$DIR/stderr`; once the test has
    /// made `$DIR/go`, the compositor killed, the component waited for, and
    /// its status written to `$DIR/status`. The wait for `go` is bounded by
    /// the patience the test gives in `$PATIENCE`, so that a test that fails
    /// first does not leave the session waiting.
    const SCRIPT: &str = r#""$0" "$@" 2> "$DIR/stderr" > /dev/null &
pid=$!
waited=0
until [ -e "$DIR/go" ]; do
  waited=$((waited + 1))
  if [ "$waited" -gt $((PATIENCE * 10)) ]; then kill "$pid"; exit 1; fi
  sleep 0.1
done
kill "$DESKRINCH_SESSION_COMPOSITOR_PID"
wait "$pid"
echo "$?" > "$DIR/status""#;

    /// The run's compositor, as `cargo xtask compositor-tests` names it.
    fn compositor() -> String {
        std::env::var("DESKRINCH_TEST_COMPOSITOR")
            .expect("run by cargo xtask compositor-tests (DESKRINCH_TEST_COMPOSITOR)")
    }

    /// What the test waits for before the compositor is killed: the
    /// component's first frame of this kind, of any window.
    #[derive(Clone, Copy, Debug)]
    enum First {
        /// A presented frame: a component with a window shown.
        Presented,
        /// A prepared frame: a component that starts hidden.
        Prepared,
    }

    impl First {
        /// Whether the log at `path` has such a frame line.
        fn in_log(self, path: &Path) -> bool {
            fs::read_to_string(path).is_ok_and(|log| {
                log.lines().any(|line| {
                    matches!(
                        (self, parse(line)),
                        (Self::Presented, Some(PerfLine::Frame { .. }))
                            | (Self::Prepared, Some(PerfLine::Prepared { .. }))
                    )
                })
            })
        }
    }

    /// The component `package` with `args`, in a session of its own whose
    /// compositor is killed once the component has drawn its `first`
    /// frame: its exit status and what it wrote to stderr.
    fn ended_with_its_display(package: &str, args: &[&str], first: First) -> (i32, String) {
        let dir = tempfile::tempdir().unwrap();
        let component = program(package).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command
            .args(["session", &compositor(), "sh", "-c", SCRIPT])
            .arg(&component)
            .args(args)
            .env("DIR", dir.path())
            .env("PATIENCE", PATIENCE.as_secs().to_string())
            .env("RINCH_PERF", "1")
            .env("RINCH_RENDERER", "software")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut session = deskrinch_test_lifetime::spawn(command).unwrap();
        let stderr = dir.path().join("stderr");
        eventually(&format!("the component's first frame ({first:?})"), || {
            first.in_log(&stderr)
        });
        fs::write(dir.path().join("go"), "").unwrap();
        let ended = wait_ended(&mut session);
        assert!(ended.success(), "the session ended {ended}");
        let status = fs::read_to_string(dir.path().join("status")).unwrap();
        let said = fs::read_to_string(&stderr).unwrap();
        (status.trim().parse().unwrap(), said)
    }

    /// The session's status once it has ended, within the patience.
    fn wait_ended(session: &mut Child) -> std::process::ExitStatus {
        let deadline = Deadline::patience();
        loop {
            if let Some(status) = session.try_wait().unwrap() {
                return status;
            }
            assert!(!deadline.passed(), "the session did not end");
            std::thread::sleep(deskrinch_test_wait::POLL);
        }
    }

    #[track_caller]
    fn ends_with_display_lost(package: &str, args: &[&str], first: First) {
        let (status, said) = ended_with_its_display(package, args, first);
        assert_eq!(
            status,
            i32::from(DISPLAY_LOST),
            "{package} ended with {status}; it said:\n{said}"
        );
        assert!(
            said.contains(DISPLAY_LOST_LINE),
            "{package} did not say {DISPLAY_LOST_LINE:?}; it said:\n{said}"
        );
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_wallpaper_ends_with_display_lost() {
        ends_with_display_lost("deskrinch-wallpaper", &[], First::Presented);
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_panel_ends_with_display_lost() {
        ends_with_display_lost("deskrinch-panel", &[], First::Presented);
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_launcher_shown_ends_with_display_lost() {
        ends_with_display_lost("deskrinch-launcher", &["--show"], First::Presented);
    }

    /// The launcher as it nearly always is, and as it is when a compositor
    /// it was a client of crashes: hidden, its frame prepared but never
    /// presented.
    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_launcher_hidden_ends_with_display_lost() {
        ends_with_display_lost("deskrinch-launcher", &[], First::Prepared);
    }
}

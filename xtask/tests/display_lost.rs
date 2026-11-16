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
//! or prepared for the launcher started hidden; for the notification
//! daemon, for its name owned on the session's bus (with no window), or for
//! its popup, sent once it owns it. It then lets the session's
//! command kill the compositor (`DESKRINCH_SESSION_COMPOSITOR_PID`), wait
//! for the component and write its status. `xtask session` waits for its
//! command whatever happens to the compositor, so the status and the last
//! lines come out before the session ends.

#[cfg(test)]
mod lost {
    use std::fs;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};

    use deskrinch_shell::exit_status::{DISPLAY_LOST, DISPLAY_LOST_LINE};
    use deskrinch_shell::{NOTIFICATIONS, Ready};
    use deskrinch_test_component::perf::{PerfLine, parse};
    use deskrinch_test_component::programs::program;
    use deskrinch_test_wait::{Deadline, PATIENCE, eventually};

    /// The session's command: the component (`$0`, its arguments after) in
    /// the background with its stderr in `$DIR/stderr`; once the test has
    /// made `$DIR/go`, the compositor killed, the component waited for, and
    /// its status written to `$DIR/status`. The wait for `go` is bounded by
    /// the patience the test gives in `$PATIENCE`, so that a test that fails
    /// first does not leave the session waiting.
    ///
    /// Meanwhile, on the session's own bus, which the test cannot reach:
    /// `$DIR/owned` is made once the name `$NAME` (when given) is owned,
    /// and once the test has made `$DIR/send` a notification is sent, its
    /// reply in `$DIR/sent`.
    const SCRIPT: &str = r#""$0" "$@" 2> "$DIR/stderr" > /dev/null &
pid=$!
waited=0
until [ -e "$DIR/go" ]; do
  if [ -n "$NAME" ] && [ ! -e "$DIR/owned" ] && busctl --user status "$NAME" > /dev/null 2>&1; then
    touch "$DIR/owned"
  fi
  if [ -e "$DIR/send" ] && [ ! -e "$DIR/sent" ]; then
    busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications \
      org.freedesktop.Notifications Notify 'susssasa{sv}i' Test 0 '' Shown 'a popup' 0 0 0 \
      > "$DIR/sent" 2>&1
  fi
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

    /// What the test waits for before the compositor is killed.
    #[derive(Clone, Copy, Debug)]
    enum First {
        /// A presented frame, of any window: a component with a window
        /// shown.
        Presented,
        /// A prepared frame: a component that starts hidden.
        Prepared,
        /// Its bus name owned, with no window at all: the notification
        /// daemon with nothing to show.
        Owned(&'static str),
        /// Its bus name owned, then a notification sent, then a presented
        /// frame: the notification daemon showing a popup.
        ShownAfterOwned(&'static str),
    }

    impl First {
        /// The name the session's command watches for, if any.
        fn name(self) -> &'static str {
            match self {
                Self::Owned(name) | Self::ShownAfterOwned(name) => name,
                Self::Presented | Self::Prepared => "",
            }
        }

        /// Whether the session in `dir` has come that far.
        fn reached(self, dir: &Path) -> bool {
            let frame = |wanted: fn(&PerfLine) -> bool| {
                fs::read_to_string(dir.join("stderr")).is_ok_and(|log| {
                    log.lines()
                        .any(|line| parse(line).is_some_and(|l| wanted(&l)))
                })
            };
            let presented = |l: &PerfLine| matches!(l, PerfLine::Frame { .. });
            match self {
                Self::Presented => frame(presented),
                Self::Prepared => frame(|l| matches!(l, PerfLine::Prepared { .. })),
                Self::Owned(_) => dir.join("owned").exists(),
                Self::ShownAfterOwned(_) => {
                    if dir.join("owned").exists() && !dir.join("send").exists() {
                        fs::write(dir.join("send"), "").unwrap();
                    }
                    frame(presented)
                }
            }
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
            .env("NAME", first.name())
            .env("PATIENCE", PATIENCE.as_secs().to_string())
            .env("RINCH_PERF", "1")
            .env("RINCH_RENDERER", "software")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut session = deskrinch_test_lifetime::spawn(command).unwrap();
        let stderr = dir.path().join("stderr");
        eventually(
            &format!("the component to come as far as {first:?}"),
            || first.reached(dir.path()),
        );
        if matches!(first, First::Owned(_)) {
            let log = fs::read_to_string(&stderr).unwrap_or_default();
            assert!(
                !log.lines()
                    .any(|line| matches!(parse(line), Some(PerfLine::Opened(_)))),
                "a window with nothing to show:\n{log}"
            );
        }
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

    /// The notification daemon with nothing to show has no window; rinch's
    /// event loop holds the display all the same, and sees it go.
    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_notification_daemon_with_no_window_ends_with_display_lost() {
        let name = notifications_name();
        ends_with_display_lost(NOTIFICATIONS.package, &[], First::Owned(name));
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_notification_daemon_showing_a_popup_ends_with_display_lost() {
        let name = notifications_name();
        ends_with_display_lost(NOTIFICATIONS.package, &[], First::ShownAfterOwned(name));
    }

    /// The name the shell waits for the notification daemon to own.
    fn notifications_name() -> &'static str {
        match NOTIFICATIONS.ready {
            Ready::BusName(name) => name,
            Ready::Started => panic!("the shell waits for the daemon's name"),
        }
    }
}

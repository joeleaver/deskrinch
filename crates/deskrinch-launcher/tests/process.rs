//! The `deskrinch-launcher` binary against a real compositor: nested KWin,
//! sway or labwc, started by `cargo xtask compositor-tests`, which runs these
//! one at a time inside the session (its own display and bus).
//!
//! What is checked is the process's behaviour, through what it can be seen
//! doing without looking at the screen: who owns the launcher's name on the
//! bus, what the bus and `deskrinch-launcher toggle|show|hide` answer, how
//! the processes exit, and the frames the launcher presents (rinch's
//! `RINCH_PERF` lines: a hidden launcher presents none, a shown one does).
//! Whether the surface is on the screen, and where, is for the screen tests.
//!
//! Every launcher a test starts is ended before the test returns, the one a
//! client starts (in a process group of its own) included.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod process {
    use std::io::{BufRead, BufReader, Read};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use deskrinch_bus::Bus;
    use deskrinch_launcher::{Answer, NAME, Request, send};
    use rustix::process::{Pid, Signal, kill_process};

    /// How long something that should happen gets.
    const PATIENCE: Duration = Duration::from_secs(30);
    /// How long a test watches for something that must not happen.
    const QUIET: Duration = Duration::from_secs(2);

    const LAUNCHER: &str = env!("CARGO_BIN_EXE_deskrinch-launcher");

    /// Panics unless the tests run in a nested session started by
    /// `cargo xtask compositor-tests`, which sets `DESKRINCH_TEST_COMPOSITOR`.
    /// Run anywhere else, they would take the launcher's name on the real
    /// session bus and map an overlay with the keyboard on the real
    /// compositor. Every test calls it first, before it touches the bus or
    /// starts a process.
    ///
    /// The window-management tests' `Compositor::current` is the same guard,
    /// in their test support, where it cannot be shared; this becomes
    /// `deskrinch_test_client::Compositor::current()` with the launcher's
    /// `SUITES` entry, once `deskrinch-test-client` is on `main`.
    fn in_nested_session() {
        assert!(
            matches!(
                std::env::var("DESKRINCH_TEST_COMPOSITOR").as_deref(),
                Ok("kwin" | "sway" | "labwc")
            ),
            "the launcher's process tests run in a nested compositor: \
             use `cargo xtask compositor-tests`"
        );
    }

    /// Counts a process's presented frames from its standard error.
    fn count_frames(stream: impl Read + Send + 'static) -> Arc<AtomicU64> {
        let frames = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&frames);
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if line.starts_with("[PERF] frame ") {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }
        });
        frames
    }

    /// A launcher this test started; ended when dropped.
    struct Launcher {
        child: Child,
        frames: Arc<AtomicU64>,
    }

    impl Launcher {
        /// `deskrinch-launcher` with `args`, once it owns the name.
        fn start(args: &[&str]) -> Self {
            let mut child = launcher(args).spawn().unwrap();
            let frames = count_frames(child.stderr.take().unwrap());
            let launcher = Self { child, frames };
            assert!(
                eventually(|| owner_pid() == Some(launcher.child.id())),
                "the launcher did not take its name"
            );
            launcher
        }

        fn frames(&self) -> u64 {
            self.frames.load(Ordering::SeqCst)
        }
    }

    impl Drop for Launcher {
        fn drop(&mut self) {
            self.child.kill().ok();
            self.child.wait().ok();
            // Its name goes with it, for the next test.
            assert!(eventually(|| owner_pid().is_none()));
        }
    }

    /// The launcher binary with `args`, drawing with the software renderer
    /// and logging its frames.
    fn launcher(args: &[&str]) -> Command {
        let mut command = Command::new(LAUNCHER);
        command
            .args(args)
            .env("RINCH_PERF", "1")
            .env("RINCH_RENDERER", "software")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        command
    }

    /// Runs `deskrinch-launcher` with `args` to its end.
    fn client(args: &[&str]) -> ExitStatus {
        let mut command = Command::new(LAUNCHER);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command.status().unwrap()
    }

    /// The process that owns the launcher's name on the session bus.
    fn owner_pid() -> Option<u32> {
        zbus::block_on(async {
            let connection = Bus::Session.connect().await.unwrap();
            let reply = connection
                .call_method(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    Some("org.freedesktop.DBus"),
                    "GetConnectionUnixProcessID",
                    &(NAME,),
                )
                .await
                .ok()?;
            reply.body().deserialize::<u32>().ok()
        })
    }

    fn eventually(mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        done()
    }

    fn stays(mut holds: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + QUIET;
        while Instant::now() < deadline {
            if !holds() {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        holds()
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn starts_hidden_and_draws_nothing() {
        in_nested_session();
        let launcher = Launcher::start(&[]);
        assert!(stays(|| launcher.frames() == 0), "a hidden launcher drew");
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn toggle_shows_it_and_toggles_it_back() {
        in_nested_session();
        let launcher = Launcher::start(&[]);
        assert!(client(&["toggle"]).success());
        assert!(
            eventually(|| launcher.frames() > 0),
            "a toggle did not show it"
        );
        // The next toggle hides it: the bus's own toggle then shows it again.
        assert!(client(&["toggle"]).success());
        assert_eq!(
            send(&Bus::Session, Request::Toggle).unwrap(),
            Answer::Done { shown: Some(true) }
        );
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn show_and_hide_set_it() {
        in_nested_session();
        let launcher = Launcher::start(&[]);
        assert!(client(&["show"]).success());
        assert!(eventually(|| launcher.frames() > 0));
        assert!(client(&["show"]).success());
        // Shown twice is shown: a toggle now hides it.
        assert_eq!(
            send(&Bus::Session, Request::Toggle).unwrap(),
            Answer::Done { shown: Some(false) }
        );
        assert!(client(&["show"]).success());
        assert!(client(&["hide"]).success());
        assert_eq!(
            send(&Bus::Session, Request::Toggle).unwrap(),
            Answer::Done { shown: Some(true) }
        );
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn a_second_launcher_leaves_the_first_running() {
        in_nested_session();
        let launcher = Launcher::start(&[]);
        let first = launcher.child.id();
        let second = launcher_exit(&[]);
        assert!(second.success(), "{second:?}");
        assert_eq!(owner_pid(), Some(first));
        assert!(stays(|| launcher.frames() == 0), "the first was shown");
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn a_second_launcher_asked_to_show_shows_the_first() {
        in_nested_session();
        let launcher = Launcher::start(&[]);
        let first = launcher.child.id();
        let second = launcher_exit(&["--show"]);
        assert!(second.success(), "{second:?}");
        assert_eq!(owner_pid(), Some(first));
        assert!(
            eventually(|| launcher.frames() > 0),
            "the first was not shown"
        );
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn with_none_running_hide_does_nothing() {
        in_nested_session();
        assert_eq!(owner_pid(), None);
        assert!(client(&["hide"]).success());
        assert!(stays(|| owner_pid().is_none()), "a hide started a launcher");
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn with_none_running_toggle_starts_one_shown() {
        in_nested_session();
        assert_eq!(owner_pid(), None);
        // The launcher the client starts inherits its standard error.
        let mut toggle = launcher(&["toggle"]).spawn().unwrap();
        let frames = count_frames(toggle.stderr.take().unwrap());
        assert!(toggle.wait().unwrap().success());
        assert!(eventually(|| owner_pid().is_some()), "no launcher started");
        let started = owner_pid().unwrap();
        let shown = eventually(|| frames.load(Ordering::SeqCst) > 0);
        // It runs in a process group of its own, so the session would not
        // end it: this test does, before it asserts anything more.
        let pid = Pid::from_raw(i32::try_from(started).unwrap()).unwrap();
        kill_process(pid, Signal::TERM).unwrap();
        assert!(eventually(|| owner_pid().is_none()), "it did not end");
        assert!(shown, "the started launcher was not shown");
    }

    /// Runs a second `deskrinch-launcher` with `args`, which must leave
    /// within [`PATIENCE`].
    fn launcher_exit(args: &[&str]) -> ExitStatus {
        let mut second = launcher(args).spawn().unwrap();
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(status) = second.try_wait().unwrap() {
                return status;
            }
            if Instant::now() >= deadline {
                second.kill().ok();
                second.wait().ok();
                panic!("a second launcher did not leave");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

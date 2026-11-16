//! A shell component run in a session of its own, acted on once it has
//! come far enough, and how it ended: what the tests of how components end
//! share.
//!
//! The session is `cargo xtask session` of the run's compositor, nested in
//! the one `cargo xtask compositor-tests` runs the test binary in, so that
//! ending that session's compositor does not end the suite's. Its command
//! is `cargo xtask in-session` (`xtask/src/in_session.rs`), which starts
//! the component, waits for its first frame, acts, and writes the
//! component's status; `xtask session` waits for its command whatever
//! happens to the compositor, so the status is written before the session
//! ends.

use std::fs;
use std::process::{Child, Command, ExitStatus, Stdio};

use deskrinch_test_component::programs::program;
use deskrinch_test_wait::{Deadline, PATIENCE};

/// How the component ended.
#[derive(Debug)]
pub(crate) struct Ended {
    /// Its status as a shell gives it: the exit code, or 128 and the
    /// signal's number.
    pub(crate) status: i32,
    /// What it wrote to its standard error.
    pub(crate) said: String,
    /// How much of that it had written when it was acted on.
    acted: usize,
}

impl Ended {
    /// What it said before it was acted on (the compositor ended, a
    /// signal sent).
    pub(crate) fn before(&self) -> &str {
        self.said.get(..self.acted).unwrap_or(&self.said)
    }
}

/// What to wait for before acting: `cargo xtask in-session`'s `--until`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Until {
    /// The component's first presented frame: a component with a window
    /// shown.
    Presented,
    /// Its first prepared frame: a component that starts hidden.
    Prepared,
    /// Its bus name owned on the session's bus: the notification daemon
    /// with nothing to show.
    Owned(&'static str),
    /// Its bus name owned, then a notification sent, then a presented
    /// frame: the notification daemon showing a popup.
    Notified(&'static str),
}

impl Until {
    /// The arguments it stands for.
    fn arguments(self) -> Vec<&'static str> {
        match self {
            Self::Presented => vec!["--until", "presented"],
            Self::Prepared => vec!["--until", "prepared"],
            Self::Owned(name) => vec!["--until", "owned", "--name", name],
            Self::Notified(name) => vec!["--until", "notified", "--name", name],
        }
    }
}

/// What to do then: `cargo xtask in-session`'s `--then`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Then {
    /// End the session's compositor: the component's display goes away.
    KillCompositor,
}

impl Then {
    /// The argument it stands for.
    fn argument(self) -> &'static str {
        match self {
            Self::KillCompositor => "kill-compositor",
        }
    }
}

/// The component `package` with `args`, run in a session of its own with
/// the software renderer and rinch's frame lines, and granted what the
/// shell grants it (`cargo xtask session --grant`), as on the desktop;
/// once it has drawn its first `until` frame, `then` is done. How the
/// component ended.
pub(crate) fn run(package: &str, args: &[&str], until: Until, then: Then) -> Ended {
    let dir = tempfile::tempdir().unwrap();
    let component = program(package).unwrap();
    let session_stderr = dir.path().join("session-stderr");
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .args(["session", "--grant"])
        .arg(format!("{package}={}", component.display()))
        .args([&compositor(), env!("CARGO_BIN_EXE_xtask"), "in-session"])
        .arg("--dir")
        .arg(dir.path())
        .args(until.arguments())
        .args(["--then", then.argument()])
        .arg(&component)
        .args(args)
        .env("RINCH_PERF", "1")
        .env("RINCH_RENDERER", "software")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(fs::File::create(&session_stderr).unwrap());
    let mut session = deskrinch_test_lifetime::spawn(command).unwrap();
    let ended = wait_ended(&mut session);
    let said = fs::read_to_string(dir.path().join("stderr")).unwrap_or_default();
    assert!(
        ended.success(),
        "the session ended {ended}; it said:\n{}\nthe component said:\n{said}",
        fs::read_to_string(&session_stderr).unwrap_or_default()
    );
    let read = |name: &str| fs::read_to_string(dir.path().join(name)).unwrap();
    Ended {
        status: read("status").trim().parse().unwrap(),
        acted: read("acted").trim().parse().unwrap(),
        said,
    }
}

/// The run's compositor, as `cargo xtask compositor-tests` names it.
fn compositor() -> String {
    std::env::var("DESKRINCH_TEST_COMPOSITOR")
        .expect("run by cargo xtask compositor-tests (DESKRINCH_TEST_COMPOSITOR)")
}

/// The session's status once it has ended: within the patience for the
/// component's first frame, and the patience again for its end after the
/// action (`cargo xtask in-session` waits for each that long), with a
/// patience to spare for the session's start and end.
fn wait_ended(session: &mut Child) -> ExitStatus {
    let deadline = Deadline::after(PATIENCE * 3);
    loop {
        if let Some(status) = session.try_wait().unwrap() {
            return status;
        }
        assert!(!deadline.passed(), "the session did not end");
        std::thread::sleep(deskrinch_test_wait::POLL);
    }
}

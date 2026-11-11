//! Why a task could not run to completion.

use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

/// A task could not finish its work. A check that ran and found problems is
/// not an error: it reports [`crate::Outcome::Failed`].
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// `git` could not be started.
    #[error("could not run `git {args}`")]
    GitSpawn {
        /// The arguments `git` was given.
        args: String,
        /// Why starting it failed.
        source: io::Error,
    },
    /// `git` ran and reported failure.
    #[error("`git {args}` failed ({status}): {stderr}")]
    GitFailed {
        /// The arguments `git` was given.
        args: String,
        /// How it exited.
        status: ExitStatus,
        /// What it printed on standard error, trimmed.
        stderr: String,
    },
    /// `git` listed a path that is not valid UTF-8.
    #[error("`git {args}` listed a path that is not UTF-8")]
    NonUtf8Path {
        /// The arguments `git` was given.
        args: String,
    },
    /// A file could not be read.
    #[error("could not read {}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// Why reading it failed.
        source: io::Error,
    },
    /// A file-system operation failed.
    #[error("could not {what}")]
    Io {
        /// What was being done.
        what: String,
        /// Why it failed.
        source: io::Error,
    },
    /// A program could not be started.
    #[error("could not run `{program}` (is it installed?)")]
    Spawn {
        /// The program.
        program: String,
        /// Why starting it failed.
        source: io::Error,
    },
    /// `cargo` could not build the test binaries.
    #[error("building the compositor tests failed ({status})")]
    BuildFailed {
        /// How `cargo` exited.
        status: ExitStatus,
    },
    /// `cargo` built the tests but did not report a test binary.
    #[error("cargo reported no executable for {package}'s `{name}` test")]
    MissingTestBinary {
        /// The package.
        package: &'static str,
        /// The test target.
        name: &'static str,
    },
    /// A test binary cannot be granted what KWin restricts.
    #[error("could not write a test binary's desktop file")]
    DesktopEntry(#[from] deskrinch_kwin_access::DesktopEntryError),
    /// The nested compositor exited before opening its socket.
    #[error("{compositor} exited ({status}) before opening its socket; its log:\n{log}")]
    CompositorExited {
        /// The compositor.
        compositor: &'static str,
        /// How it exited.
        status: ExitStatus,
        /// What it printed.
        log: String,
    },
    /// The session's private D-Bus bus could not start.
    #[error("could not start the session's D-Bus bus")]
    Bus(#[from] deskrinch_test_dbus::Error),
    /// The handler that ends sessions on `SIGINT`, `SIGTERM` and `SIGHUP`
    /// could not be installed.
    #[error("could not handle signals: {reason}")]
    SignalHandler {
        /// Why.
        reason: String,
    },
    /// A signal asked xtask to stop; the sessions it ran have been ended.
    #[error("interrupted; the session was ended")]
    Interrupted,
    /// Processes of a session were still running after it was ended. They
    /// have been killed; the session's directory is kept.
    #[error(
        "processes of the session outlived it and were killed (directory kept: {}):\n{}",
        root.display(),
        leftovers.iter().map(|leftover| format!("  {leftover}")).collect::<Vec<_>>().join("\n")
    )]
    SessionLeftovers {
        /// What was still running.
        leftovers: Vec<crate::session::Leftover>,
        /// The session's directory.
        root: PathBuf,
    },
    /// The nested compositor opened its socket, but did not take one of the
    /// D-Bus names it registers at startup.
    #[error("{compositor} did not take {name} on the session's bus; its log:\n{log}")]
    CompositorNotReady {
        /// The compositor.
        compositor: &'static str,
        /// The name that did not appear.
        name: &'static str,
        /// Why waiting for it ended.
        source: Box<deskrinch_test_dbus::NameError>,
        /// What it printed.
        log: String,
    },
    /// The nested compositor's outputs could not be set up.
    #[error("could not {what} under {compositor}; its log:\n{log}")]
    OutputSetup {
        /// The compositor.
        compositor: &'static str,
        /// What was being set up.
        what: String,
        /// Why it failed.
        source: Box<deskrinch_test_client::Error>,
        /// What it printed.
        log: String,
    },
    /// The nested compositor's outputs do not have the scale asked for.
    #[error("{compositor} did not take scale {asked}: {outputs}; its log:\n{log}")]
    WrongScale {
        /// The compositor.
        compositor: &'static str,
        /// The scale asked for.
        asked: String,
        /// The outputs at another scale, and theirs.
        outputs: String,
        /// What it printed.
        log: String,
    },
    /// The program that sets the outputs' scale failed.
    #[error(
        "`{program}` could not set the outputs' scale ({status}): {output}; the compositor's log:\n{log}"
    )]
    ScaleToolFailed {
        /// The program.
        program: &'static str,
        /// How it exited.
        status: ExitStatus,
        /// What it printed.
        output: String,
        /// What the compositor printed.
        log: String,
    },
    /// `ci-packages` was run outside a CI job: it installs packages with
    /// `sudo`, which nothing may do on a developer's machine.
    #[error(
        "ci-packages installs system packages and runs only in CI \
         (GITHUB_ACTIONS=true); install what you need yourself"
    )]
    NotInCi,
    /// The nested compositor did not open its socket in time.
    #[error("{compositor} did not open its socket in time; its log:\n{log}")]
    CompositorTimedOut {
        /// The compositor.
        compositor: &'static str,
        /// What it printed.
        log: String,
    },
    /// The measured program exited before the measurement was done.
    #[error("{program} exited ({status}) while it was measured; its last output:\n{output}")]
    MeasuredExited {
        /// The program.
        program: String,
        /// How it exited.
        status: ExitStatus,
        /// Its last lines of output.
        output: String,
    },
    /// The measured program drew no frame in time.
    #[error(
        "{program} presented no frame within {} s (is it a rinch program? the first \
         frame is rinch's RINCH_PERF line); its last output:\n{output}",
        waited.as_secs()
    )]
    NoFirstFrame {
        /// The program.
        program: String,
        /// How long it was given.
        waited: std::time::Duration,
        /// Its last lines of output.
        output: String,
    },
    /// The measured program's `/proc` entry went away while it was read.
    #[error("{program} went away while it was measured")]
    MeasuredGone {
        /// The program.
        program: String,
    },
    /// `callgrind_control` could not get a dump.
    #[error("callgrind_control could not dump the profile ({status})")]
    CallgrindControl {
        /// How it exited.
        status: ExitStatus,
    },
}

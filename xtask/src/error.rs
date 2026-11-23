//! Why a task could not run to completion.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use deskrinch_test_session::ci::Ci;

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
    /// The compositor tests' suites or test asked for cannot run.
    #[error(transparent)]
    Selection(#[from] crate::compositor_tests::selection::SelectionError),
    /// A repeat's directory already holds something.
    #[error(
        "{} is not empty: each repeat writes its runs' output to a directory of its own",
        .0.display()
    )]
    RepeatIntoNotEmpty(PathBuf),
    /// A key binding's program is not an absolute path.
    #[error("a key binding must run an absolute path, not {}", .0.display())]
    BindingNotAbsolute(PathBuf),
    /// A key binding's command holds a word that would need quoting.
    #[error(
        "a key binding's command would need {0:?} quoted; only letters, digits and `/._-+,:=@` \
         are written"
    )]
    BindingNeedsQuoting(OsString),
    /// The compositor did not put a key binding in force in time.
    #[error("{compositor} did not register the key binding {binding} in time; its log:\n{log}")]
    BindingNotInForce {
        /// The compositor.
        compositor: &'static str,
        /// The binding's id.
        binding: String,
        /// The compositor's output so far.
        log: crate::session::CompositorLog,
    },
    /// `cargo xtask desktop` could not run.
    #[error(transparent)]
    Desktop(#[from] crate::desktop::DesktopError),
    /// The shell's components could not be built.
    #[error(transparent)]
    Components(#[from] crate::components::BuildError),
    /// `cargo xtask input` stopped.
    #[error(transparent)]
    Input(#[from] crate::input::InputError),
    /// `cargo xtask session --grant` was not given `PACKAGE=PATH` with one of
    /// the shell's components.
    #[error("--grant {0}: give PACKAGE=PATH, PACKAGE one of the shell's components")]
    GrantNotUnderstood(String),
    /// `cargo xtask in-session` could not run its component to its end.
    #[error(transparent)]
    InSession(#[from] crate::in_session::InSessionError),
    /// `cargo xtask measure` could not type or point, or the program did not
    /// take what was typed, or drew nothing for a step.
    #[error(transparent)]
    MeasuredInput(#[from] crate::measure::MeasuredInputError),
    /// A test binary cannot be granted what KWin restricts.
    #[error("could not write a test binary's desktop file")]
    DesktopEntry(#[from] deskrinch_kwin_access::DesktopEntryError),
    /// The nested compositor exited while the session started, set up its
    /// outputs, or listed them.
    #[error("{compositor} exited ({status}) {doing}; its log:\n{log}")]
    CompositorExited {
        /// The compositor.
        compositor: &'static str,
        /// How it exited.
        status: ExitStatus,
        /// What the session was doing.
        doing: crate::session::Doing,
        /// What it printed.
        log: crate::session::CompositorLog,
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
    /// A session's supervisor was given a directory that is not a session's
    /// root, and refused to touch it.
    #[error("{} is not a session's directory ({why}); the supervisor does nothing to it", root.display())]
    NotASession {
        /// The directory it was given.
        root: PathBuf,
        /// What it is not.
        why: &'static str,
    },
    /// A session's supervisor did not say it was ready.
    #[error("the session's supervisor did not start")]
    SupervisorNotReady,
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
    #[error("{compositor} did not take {name} on the session's bus ({hint}); its log:\n{log}")]
    CompositorNotReady {
        /// The compositor.
        compositor: &'static str,
        /// The name that did not appear.
        name: &'static str,
        /// What registers it.
        hint: &'static str,
        /// Why waiting for it ended.
        source: Box<deskrinch_test_dbus::NameError>,
        /// What it printed.
        log: crate::session::CompositorLog,
    },
    /// The nested compositor could not be asked how it runs.
    #[error("could not read {what} from {compositor}; its log:\n{log}")]
    CompositorQuery {
        /// The compositor.
        compositor: &'static str,
        /// What was asked for.
        what: &'static str,
        /// Why asking failed.
        source: Box<zbus::Error>,
        /// What it printed.
        log: crate::session::CompositorLog,
    },
    /// The nested compositor started in a state the session cannot use.
    #[error("{why} ({compositor} says \"{line}\"); its log:\n{log}")]
    CompositorUnfit {
        /// The compositor.
        compositor: &'static str,
        /// What is wrong, and why it matters.
        why: &'static str,
        /// The compositor's own line that says so.
        line: String,
        /// What it printed.
        log: crate::session::CompositorLog,
    },
    /// A session was to render on the GPU, and the machine has none.
    #[error("the compositor was to render on the GPU, but this machine has none: {nodes}")]
    NoGpu {
        /// The machine's render nodes, as `RenderNodes` lists them.
        nodes: String,
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
        log: crate::session::CompositorLog,
    },
    /// `--output-size` given for a run with a session whose outputs are its
    /// pass's own.
    #[error(
        "--output-size cannot change the outputs of a pass that has its own ({outputs}): leave \
         it off, or choose suites that do not run there"
    )]
    OutputsOfItsOwn {
        /// The pass's outputs.
        outputs: String,
    },
    /// More output sizes asked for than there are outputs.
    #[error(transparent)]
    OutputSizes(#[from] crate::session::TooManySizes),
    /// The nested compositor's outputs are not as asked.
    #[error("{compositor} did not take {asked}: {outputs}; its log:\n{log}")]
    WrongOutputs {
        /// The compositor.
        compositor: &'static str,
        /// The outputs asked for.
        asked: String,
        /// The outputs not as asked, and how they are.
        outputs: String,
        /// What it printed.
        log: crate::session::CompositorLog,
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
        log: crate::session::CompositorLog,
    },
    /// A command that may run only on a CI runner was run elsewhere:
    /// `ci-packages`, which installs packages with `sudo`, and
    /// `systemd-scope-tests`, which starts a unit in the user's systemd;
    /// neither may do that on a developer's machine.
    #[error("{command} {why}")]
    NotOnCi {
        /// The command, as a person runs it.
        command: &'static str,
        /// What the CI variable held.
        why: deskrinch_test_session::ci::NotCi,
    },
    /// A variable GitHub Actions sets in every job was not set.
    #[error("the CI variable `{name}` is not set: ci-packages runs in GitHub Actions only")]
    CiVariable {
        /// The variable.
        name: &'static str,
    },
    /// `apt-get install --print-uris` printed a download it could not read.
    #[error("apt-get install --print-uris listed a download xtask cannot read: {line}")]
    AptListing {
        /// The line.
        line: String,
    },
    /// The plan `ci-packages update` leaves could not be read back.
    #[error("{} is not the plan `ci-packages update` writes: {reason}", path.display())]
    PackagePlan {
        /// The file.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// The package cache's directory could not be made.
    #[error("could not make the package cache's directory: `install -d` {ended}")]
    PackageCache {
        /// How `install` ended.
        ended: String,
    },
    /// The nested compositor did not accept a connection on its socket in
    /// time, while it ran.
    #[error(
        "{compositor} did not accept a connection on its socket in time ({why}); its log:\n{log}"
    )]
    CompositorTimedOut {
        /// The compositor.
        compositor: &'static str,
        /// The last thing seen: no socket, or the last refusal.
        why: String,
        /// What it printed.
        log: crate::session::CompositorLog,
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
    /// The measured program's start-up did not reach the end
    /// `--start-ends` names within the patience.
    #[error(
        "{program}'s start-up did not reach its end ({end}) within {} s; its last lines:\n{output}",
        waited.as_secs()
    )]
    StartNotEnded {
        /// The program.
        program: String,
        /// The end waited for, described.
        end: String,
        /// How long it was given.
        waited: std::time::Duration,
        /// Its last lines of output.
        output: String,
    },
    /// `--start-ends ready` for a program with no readiness to end at.
    #[error("--start-ends ready: {program} has no readiness to end at: {why}")]
    NoReadiness {
        /// The program.
        program: String,
        /// Why.
        why: String,
    },
    /// The measured program's windows were not those `--windows` expects:
    /// too few presented in time, too many, two of one name, or they
    /// changed after start-up.
    #[error("{program}: {source}\nIts last output:\n{output}")]
    MeasuredWindows {
        /// The program.
        program: String,
        /// What was wrong.
        source: crate::measure::WindowsError,
        /// Its last lines of output.
        output: String,
    },
    /// The measured program's `/proc` entry went away while it was read.
    #[error("{program} went away while it was measured")]
    MeasuredGone {
        /// The program.
        program: String,
    },
    /// Valgrind's Callgrind tool, which KWin is to grant under
    /// `--instructions`, could not be found by starting it.
    #[error("could not find Valgrind's Callgrind tool to grant it: {why}")]
    CallgrindToolNotFound {
        /// What happened instead.
        why: String,
    },
    /// A component named no component of the shell.
    #[error("{name} is not one of the shell's components ({known})")]
    UnknownComponent {
        /// The name given.
        name: String,
        /// The components' packages.
        known: String,
    },
    /// The measured program logged at error level: a program that says it
    /// is broken is not measured (unless `--allow-errors`).
    #[error(
        "{program} logged {count} error(s), so the run is not a measurement (its output is in \
         {}; --allow-errors measures it anyway):\n{lines}",
        log.display()
    )]
    MeasuredLoggedErrors {
        /// The program.
        program: String,
        /// How many error lines.
        count: usize,
        /// The error lines.
        lines: String,
        /// The file with all its output.
        log: PathBuf,
    },
    /// `--window`'s window could not be opened: no run is reported without
    /// it.
    #[error("the window with app id {app_id} could not be opened at the idle window's start")]
    WindowNotOpened {
        /// The window's app id.
        app_id: String,
        /// Why.
        source: Box<deskrinch_test_client::Error>,
    },
    /// `--window`'s window was gone by the end of the idle window (its
    /// connection ended, or the compositor asked it to close): the figures
    /// would not be what one window costs.
    #[error("the window with app id {app_id} was gone by the end of the idle window ({why})")]
    WindowGone {
        /// The window's app id.
        app_id: String,
        /// What happened to it.
        why: &'static str,
    },
    /// `callgrind_control` could not get a dump.
    #[error("callgrind_control could not dump the profile ({status})")]
    CallgrindControl {
        /// How it exited.
        status: ExitStatus,
    },
    /// With `--summon`, the program drew a frame before it was summoned.
    #[error(
        "{program} presented a frame before it was summoned: with --summon it must start \
         hidden; its last output:\n{output}"
    )]
    ShownBeforeSummon {
        /// The program.
        program: String,
        /// Its last lines of output.
        output: String,
    },
    /// With `--summon`, the program did not settle after starting or after
    /// a show or hide.
    #[error("{program} did not settle within {} s; its last output:\n{output}", waited.as_secs())]
    NeverSettled {
        /// The program.
        program: String,
        /// How long it was given.
        waited: std::time::Duration,
        /// Its last lines of output.
        output: String,
    },
    /// Two dumps of one Callgrind profile carry the same hint, so which one
    /// a figure is cannot be told.
    #[error(
        "the Callgrind profile has {} dumps with the hint {label:?}, where a run makes each \
         hint once: {}",
        files.len(),
        files.iter().map(|file| file.display().to_string()).collect::<Vec<_>>().join(", ")
    )]
    CallgrindHintRepeated {
        /// The hint.
        label: String,
        /// The dumps that carry it.
        files: Vec<PathBuf>,
    },
    /// A program said to prepare its hidden window (`--prepares`) never
    /// did.
    #[error(
        "{program} prepared nothing within {} s, though --prepares says it prepares its hidden \
         window; its last output:\n{output}",
        waited.as_secs()
    )]
    NeverPrepared {
        /// The program.
        program: String,
        /// How long it was given.
        waited: std::time::Duration,
        /// Its last lines of output.
        output: String,
    },
    /// The command that summons or dismisses the program failed.
    #[error("`{command}` failed ({status})")]
    SummonFailed {
        /// The command, as run.
        command: String,
        /// How it exited.
        status: ExitStatus,
    },
}

/// Goes on only on a CI runner, as `ci` says ([`Ci::only_on_a_runner`]);
/// refuses `command` with [`Error::NotOnCi`] anywhere else.
pub(crate) fn only_on_a_runner(command: &'static str, ci: &Ci) -> Result<(), Error> {
    ci.only_on_a_runner()
        .map_err(|why| Error::NotOnCi { command, why })
}

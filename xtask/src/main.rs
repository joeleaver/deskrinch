//! Repository tasks for deskrinch, run as `cargo xtask <task>`.
//!
//! Each task lives in a module of its own that exposes a `run` function
//! returning [`Outcome`]. Adding a task is a new module, one [`Task`] variant
//! and one match arm in [`Task::run`]; existing tasks are not touched.

mod busy;
mod cargo;
mod cargo_messages;
mod ci_packages;
mod components;
mod compositor_tests;
mod desktop;
mod error;
mod file_length;
mod git;
mod grant;
mod in_session;
mod input;
mod measure;
mod run_in_session;
mod session;
mod systemd_scope_tests;

use std::error::Error as _;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::error::Error;

/// The repository root: the parent of this crate's directory.
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// Command line of `cargo xtask`.
#[derive(Debug, Parser)]
#[command(name = "cargo xtask", bin_name = "cargo xtask", about)]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

/// The repository's tasks.
#[derive(Debug, Subcommand)]
enum Task {
    /// Fail when a Rust source file tracked by git is too long.
    #[command(about = format!(
        "Fail when a Rust source file tracked by git is longer than {} lines",
        file_length::MAX_LINES,
    ))]
    FileLength,
    /// Build the shell's components, as the desktop and the compositor tests
    /// build them, and print PACKAGE's binary's path, or without it each
    /// package and its binary's path, a tab between, one a line.
    ///
    /// One `cargo build` of every component and what the key bindings run,
    /// unoptimised unless `--release`: the one place the components are
    /// built, so that a binary measured or installed is the one the tests
    /// and the desktop run.
    BuildComponents {
        /// Build them with Cargo's release profile.
        #[arg(long)]
        release: bool,
        /// Print only this component's binary's path (all are built still).
        package: Option<String>,
    },
    /// Run the workspace's compositor tests in nested, headless
    /// compositors.
    ///
    /// The test binaries are those of the suites table
    /// (`xtask/src/compositor_tests/suites.rs`), each with the compositors
    /// and the passes it runs in: a pass is one isolated session per
    /// compositor (the one `cargo xtask session` describes) with its outputs
    /// set up a given way, at scale 1, at scale 2, turned or flipped. Under
    /// KWin each binary is granted what it uses by a desktop file. Nothing
    /// reaches the real desktop session. Needs `dbus-daemon`, and
    /// `kwin_wayland` (with `kscreen-doctor`), `sway` and `labwc` for those
    /// compositors.
    CompositorTests(compositor_tests::Args),
    /// Run a command in an isolated, nested, headless compositor session.
    #[command(long_about = run_in_session::long_about())]
    Session {
        /// How many outputs the compositor has, side by side, 1280x720 each
        /// unless `--output-size` gives another.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=8))]
        outputs: u32,
        #[command(flatten)]
        sizes: session::SizeArgs,
        /// The outputs' scale from the start, a positive decimal such as 2 or
        /// 1.5: physical pixels per logical one, each output's mode still
        /// its size (1280x720, or `--output-size`'s). sway gets it in its
        /// configuration (`output * scale`); labwc, which has no such
        /// setting, through wlr-output-management, and KWin through
        /// `kscreen-doctor` (needed then), both before the
        /// command starts. The session fails (70) if an output then reports
        /// another scale (its whole scale, or a logical size that is not its
        /// mode over the scale).
        #[arg(long, default_value = "1", value_name = "SCALE")]
        scale: session::Scale,
        /// The outputs' transform from the start, as `wl_output` names it
        /// (rotations counter-clockwise): normal, 90, 180, 270, flipped,
        /// flipped-90, flipped-180 or flipped-270. sway gets it in its
        /// configuration, labwc through wlr-output-management and KWin
        /// through `kscreen-doctor`, as the scale; the session fails (70) if
        /// an output then reports another.
        #[arg(long, default_value = "normal", value_name = "TRANSFORM")]
        transform: session::OutputTransform,
        /// Keep the session's directory afterwards (its compositor log, and
        /// whatever the command wrote to `$DESKRINCH_SESSION_DIR`).
        #[arg(long)]
        keep: bool,
        /// `XDG_CURRENT_DESKTOP` for the compositor and the command (unset
        /// otherwise), such as `KDE` or `sway`.
        #[arg(long, value_name = "NAME")]
        current_desktop: Option<OsString>,
        /// What the compositor renders with: `software` (wlroots' pixman
        /// renderer for sway and labwc), or `gpu` (wlroots' GLES2 renderer on
        /// the GPU's render node, which a client presenting through Vulkan
        /// needs). KWin composites with OpenGL either way. `gpu` fails (70)
        /// where the machine has no GPU, and when the compositor does not say
        /// it renders on it.
        #[arg(long, value_enum, default_value = "software")]
        compositor_renderer: session::CompositorRenderer,
        /// Run as on a machine without a GPU: the session's programs are told
        /// the run uses none (`DESKRINCH_SESSION_GPU=none`), as `cargo xtask
        /// compositor-tests --renderer software` tells them, so that `cargo
        /// xtask desktop` inside composites in software as it does in CI.
        #[arg(long)]
        no_gpu: bool,
        /// Grant the shell's component PACKAGE, run from the executable
        /// PATH, what deskrinch-shell grants it, as the desktop and measure
        /// do (repeatable). Under KWin its desktop file is installed in the
        /// session's data directory before KWin starts; sway and labwc
        /// restrict nothing, and it does nothing there.
        #[arg(long = "grant", value_name = "PACKAGE=PATH")]
        grants: Vec<String>,
        /// The compositor to run.
        #[arg(value_enum)]
        compositor: session::Compositor,
        /// The program to run in the session.
        program: OsString,
        /// Its arguments.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Open the shell in a window on the real desktop, in a nested
    /// compositor, isolated like a session. For the owner, from a terminal.
    #[command(long_about = desktop::LONG_ABOUT)]
    Desktop(desktop::Args),
    /// Measure what a rinch program (a shell component) costs, in isolated
    /// sessions: CPU time to its first frame, memory, idle wake-ups and CPU
    /// per frame, or with `--instructions` instruction counts.
    #[command(long_about = measure::LONG_ABOUT)]
    Measure(Box<measure::Args>),
    /// Install Ubuntu packages a CI job needs, from GitHub's cache where it
    /// can, failing fast when the package servers cannot be reached.
    ///
    /// `update` runs `sudo apt-get update` and names the package cache
    /// after the files apt would fetch; `install`, after the cache is
    /// restored, runs `sudo apt-get install --yes --no-install-recommends`
    /// into it. Each apt run has apt's retry and request timeouts and a
    /// limit of its own, and a server that could not be reached, or one
    /// that served too slowly, is reported as a GitHub annotation that says
    /// so. It refuses to run anywhere but in GitHub Actions
    /// (`GITHUB_ACTIONS=true`): it never installs packages on a developer's
    /// machine.
    CiPackages {
        /// The step.
        #[command(subcommand)]
        command: ci_packages::Command,
    },
    /// Run the desktop-entries test that launches into a real transient
    /// systemd scope.
    ///
    /// Uses the systemd user instance of the current session: it starts
    /// only the test's own recorder script, in a scope it stops again.
    SystemdScopeTests,
    /// The supervisor of a session that xtask starts for each one it runs:
    /// it ends the session under `root` should xtask die without ending it.
    /// Not for running by hand.
    #[command(name = session::SUPERVISOR, hide = true)]
    SessionSupervisor {
        /// The session's root directory.
        root: PathBuf,
    },
    /// The input client `measure` drives in its nested session: takes the
    /// step (a key typed, the pointer moved) each line of its input names.
    /// Refuses outside a nested session. Not for running by hand.
    #[command(name = "input", hide = true)]
    Input,
    /// The session's side of a test that runs a component in a nested
    /// session: starts it, waits for its first frame, acts on it (ends the
    /// compositor), and writes how it ended. Refuses outside a nested
    /// session. Not for running by hand.
    #[command(name = "in-session", hide = true)]
    InSession(in_session::Args),
    /// Keeps the nested session's compositor busy with a fixed amount of
    /// Wayland work, and writes when it had all been answered: a helper
    /// for measure's tests. Refuses outside a nested session. Not for
    /// running by hand.
    #[command(name = "busy-compositor", hide = true)]
    BusyCompositor(busy::Args),
}

impl Task {
    fn run(self, root: &Path) -> Result<Outcome, Error> {
        match self {
            Self::FileLength => file_length::run(root),
            Self::BuildComponents { release, package } => {
                components::run(root, release, package.as_deref())
            }
            Self::CiPackages { command } => ci_packages::run(&command),
            Self::CompositorTests(args) => compositor_tests::run(root, &args),
            Self::SystemdScopeTests => systemd_scope_tests::run(root),
            Self::Desktop(args) => desktop::run(root, &args),
            Self::SessionSupervisor { root } => session::supervise(&root).map(|()| Outcome::Passed),
            Self::Input => input::run(),
            Self::InSession(args) => in_session::run(&args),
            Self::BusyCompositor(args) => busy::run(&args),
            Self::Measure(args) => measure::run(&args.request()?),
            Self::Session {
                outputs,
                sizes,
                scale,
                transform,
                keep,
                current_desktop,
                compositor_renderer,
                no_gpu,
                grants,
                compositor,
                program,
                args,
            } => run_in_session::run(&run_in_session::Request {
                grants: grants
                    .iter()
                    .map(|asked| grant::ComponentGrant::parse(asked))
                    .collect::<Result<_, _>>()?,
                compositor,
                compositor_renderer,
                outputs: session::Outputs::new(outputs, scale, transform).sized(&sizes.sizes)?,
                keep,
                current_desktop,
                gpu: session::Gpu::from_allowed(!no_gpu),
                program,
                args,
            }),
        }
    }
}

/// How a task that ran to completion ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The repository meets the task's check.
    Passed,
    /// The repository fails the check; the task has printed why.
    Failed,
    /// The task ran a command, which exited with this status.
    Exit(u8),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runs_a_command = matches!(cli.task, Task::Session { .. } | Task::Desktop(_));
    match cli.task.run(Path::new(WORKSPACE_ROOT)) {
        Ok(Outcome::Passed) => ExitCode::SUCCESS,
        Ok(Outcome::Failed) => ExitCode::FAILURE,
        Ok(Outcome::Exit(code)) => ExitCode::from(code),
        Err(Error::Interrupted) => {
            eprintln!("error: {}", Error::Interrupted);
            ExitCode::from(run_in_session::INTERRUPTED)
        }
        Err(Error::Desktop(refused @ desktop::DesktopError::NotInteractive)) => {
            eprintln!("error: {refused}");
            ExitCode::from(desktop::REFUSED)
        }
        Err(error) => {
            eprintln!("error: {error}");
            let mut cause = error.source();
            while let Some(inner) = cause {
                eprintln!("  caused by: {inner}");
                cause = inner.source();
            }
            // A command's own status must not be mistaken for the session's
            // failure.
            if runs_a_command {
                ExitCode::from(run_in_session::SESSION_FAILED)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

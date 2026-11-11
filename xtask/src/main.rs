//! Repository tasks for deskrinch, run as `cargo xtask <task>`.
//!
//! Each task lives in a module of its own that exposes a `run` function
//! returning [`Outcome`]. Adding a task is a new module, one [`Task`] variant
//! and one match arm in [`Task::run`]; existing tasks are not touched.

mod compositor_tests;
mod error;
mod file_length;
mod git;
mod measure;
mod run_in_session;
mod session;
mod systemd_scope_tests;

use std::error::Error as _;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

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
    /// Run the window-management compositor tests in nested, headless
    /// compositors.
    ///
    /// Each compositor runs in an isolated session of its own, the one
    /// `cargo xtask session` describes, so nothing reaches the real desktop
    /// session. Needs `dbus-daemon`, and `kwin_wayland`, `sway` and `labwc`
    /// for those compositors.
    CompositorTests {
        /// The compositors to run in; all of them when none is given.
        #[arg(value_enum)]
        compositors: Vec<session::Compositor>,
    },
    /// Run a command in an isolated, nested, headless compositor session.
    #[command(long_about = run_in_session::long_about())]
    Session {
        /// How many outputs the compositor has, side by side, 1280x720 each.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=8))]
        outputs: u32,
        /// The outputs' scale from the start, a positive decimal such as 2 or
        /// 1.5: physical pixels per logical one, each output's mode still
        /// 1280x720. sway gets it in its configuration (`output * scale`);
        /// labwc, which has no such setting, through wlr-output-management,
        /// and KWin through `kscreen-doctor` (needed then), both before the
        /// command starts. The session fails (70) if an output then reports
        /// another scale.
        #[arg(long, default_value = "1", value_name = "SCALE")]
        scale: session::Scale,
        /// Keep the session's directory afterwards (its compositor log, and
        /// whatever the command wrote to `$DESKRINCH_SESSION_DIR`).
        #[arg(long)]
        keep: bool,
        /// `XDG_CURRENT_DESKTOP` for the compositor and the command (unset
        /// otherwise), such as `KDE` or `sway`.
        #[arg(long, value_name = "NAME")]
        current_desktop: Option<OsString>,
        /// The compositor to run.
        #[arg(value_enum)]
        compositor: session::Compositor,
        /// The program to run in the session.
        program: OsString,
        /// Its arguments.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Measure what a rinch program (a shell component) costs, in isolated
    /// sessions: CPU time to its first frame, memory, idle wake-ups and CPU
    /// per frame, or with `--instructions` instruction counts.
    #[command(long_about = measure::LONG_ABOUT)]
    Measure {
        /// The compositor each run's session has.
        #[arg(long, value_enum, default_value = "kwin")]
        compositor: session::Compositor,
        /// How many runs, each in a fresh session.
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=50))]
        runs: u32,
        /// The renderer, through `RINCH_RENDERER` (unset: the caller's
        /// environment decides).
        #[arg(long, value_enum)]
        renderer: Option<measure::Renderer>,
        /// The GPU driver's shader cache at each start.
        #[arg(long, value_enum, default_value = "cold")]
        shader_cache: measure::ShaderCache,
        /// Seconds after the first frame before memory is read.
        #[arg(long, default_value_t = 5)]
        settle: u64,
        /// Seconds of the idle window. Two minutes catch two ticks of a
        /// clock that shows minutes.
        #[arg(long, default_value_t = 125)]
        idle: u64,
        /// Seconds to wait for the first frame (default 120, or 1200 with
        /// `--instructions`).
        #[arg(long)]
        first_frame_timeout: Option<u64>,
        /// Count instructions under Callgrind instead of measuring CPU time
        /// and memory.
        #[arg(long)]
        instructions: bool,
        /// With `--instructions`: keep each run's Callgrind profile in
        /// `DIR/run-N/`, for `callgrind_annotate`.
        #[arg(long, value_name = "DIR", requires = "instructions")]
        profiles: Option<PathBuf>,
        /// The program to measure.
        program: OsString,
        /// Its arguments.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Run the desktop-entries test that launches into a real transient
    /// systemd scope.
    ///
    /// Uses the systemd user instance of the current session: it starts
    /// only the test's own recorder script, in a scope it stops again.
    SystemdScopeTests,
}

impl Task {
    fn run(self, root: &Path) -> Result<Outcome, Error> {
        match self {
            Self::FileLength => file_length::run(root),
            Self::CompositorTests { compositors } => compositor_tests::run(root, &compositors),
            Self::SystemdScopeTests => systemd_scope_tests::run(root),
            Self::Measure {
                compositor,
                runs,
                renderer,
                shader_cache,
                settle,
                idle,
                first_frame_timeout,
                instructions,
                profiles,
                program,
                args,
            } => measure::run(&measure::Request {
                compositor,
                runs,
                renderer,
                shader_cache,
                settle: Duration::from_secs(settle),
                idle: Duration::from_secs(idle),
                first_frame_timeout: Duration::from_secs(
                    first_frame_timeout.unwrap_or(if instructions { 1200 } else { 120 }),
                ),
                instructions,
                profiles,
                program,
                args,
            }),
            Self::Session {
                outputs,
                scale,
                keep,
                current_desktop,
                compositor,
                program,
                args,
            } => run_in_session::run(&run_in_session::Request {
                compositor,
                outputs,
                scale,
                keep,
                current_desktop,
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
    let runs_a_command = matches!(cli.task, Task::Session { .. });
    match cli.task.run(Path::new(WORKSPACE_ROOT)) {
        Ok(Outcome::Passed) => ExitCode::SUCCESS,
        Ok(Outcome::Failed) => ExitCode::FAILURE,
        Ok(Outcome::Exit(code)) => ExitCode::from(code),
        Err(Error::Interrupted) => {
            eprintln!("error: {}", Error::Interrupted);
            ExitCode::from(run_in_session::INTERRUPTED)
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

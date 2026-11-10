//! Repository tasks for deskrinch, run as `cargo xtask <task>`.
//!
//! Each task lives in a module of its own that exposes a `run` function
//! returning [`Outcome`]. Adding a task is a new module, one [`Task`] variant
//! and one match arm in [`Task::run`]; existing tasks are not touched.

mod compositor_tests;
mod error;
mod file_length;
mod git;
mod run_in_session;
mod session;
mod systemd_scope_tests;

use std::error::Error as _;
use std::ffi::OsString;
use std::path::Path;
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
    #[command(long_about = run_in_session::LONG_ABOUT)]
    Session {
        /// How many outputs the compositor has, side by side, 1280x720 each.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=8))]
        outputs: u32,
        /// Keep the session's directory afterwards (its compositor log, and
        /// whatever the command wrote to `$DESKRINCH_SESSION_DIR`).
        #[arg(long)]
        keep: bool,
        /// The compositor to run.
        #[arg(value_enum)]
        compositor: session::Compositor,
        /// The program to run in the session.
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
            Self::Session {
                outputs,
                keep,
                compositor,
                program,
                args,
            } => run_in_session::run(&run_in_session::Request {
                compositor,
                outputs,
                keep,
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

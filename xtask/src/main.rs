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
    /// Each compositor runs in a session of its own (temporary runtime,
    /// data, configuration and cache directories, and a private D-Bus bus
    /// for KWin), so nothing reaches the real desktop session. Needs
    /// `kwin_wayland` and `dbus-run-session` for KWin, `sway` for sway.
    CompositorTests {
        /// The compositors to run in; all of them when none is given.
        #[arg(value_enum)]
        compositors: Vec<session::Compositor>,
    },
    /// Run a command in a nested, headless compositor session.
    ///
    /// The session is isolated as the compositor tests' are, and the
    /// command runs as its client (`WAYLAND_DISPLAY`, and `SWAYSOCK` under
    /// sway, point at it) in its process group: when the command exits, the
    /// session ends with everything the command left running. For running a
    /// shell component by hand, for example
    /// `cargo xtask session sway sh -c 'deskrinch-panel & sleep 2; grim out.png'`.
    Session {
        /// The compositor to run.
        #[arg(value_enum)]
        compositor: session::Compositor,
        /// The program to run in the session.
        program: OsString,
        /// Its arguments.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
}

impl Task {
    fn run(self, root: &Path) -> Result<Outcome, Error> {
        match self {
            Self::FileLength => file_length::run(root),
            Self::CompositorTests { compositors } => compositor_tests::run(root, &compositors),
            Self::Session {
                compositor,
                program,
                args,
            } => run_in_session::run(compositor, &program, &args),
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.task.run(Path::new(WORKSPACE_ROOT)) {
        Ok(Outcome::Passed) => ExitCode::SUCCESS,
        Ok(Outcome::Failed) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: {error}");
            let mut cause = error.source();
            while let Some(inner) = cause {
                eprintln!("  caused by: {inner}");
                cause = inner.source();
            }
            ExitCode::FAILURE
        }
    }
}

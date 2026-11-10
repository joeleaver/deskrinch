//! Repository tasks for deskrinch, run as `cargo xtask <task>`.
//!
//! Each task lives in a module of its own that exposes a `run` function
//! returning [`Outcome`]. Adding a task is a new module, one [`Task`] variant
//! and one match arm in [`Task::run`]; existing tasks are not touched.

mod error;
mod file_length;
mod git;

use std::error::Error as _;
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
}

impl Task {
    fn run(self, root: &Path) -> Result<Outcome, Error> {
        match self {
            Self::FileLength => file_length::run(root),
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

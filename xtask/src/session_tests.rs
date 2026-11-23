//! `cargo xtask session-tests`: xtask's own tests of `cargo xtask session`
//! and `measure` against a real compositor, which start their isolated
//! sessions themselves through xtask's binary. The list of them has its one
//! home here ([`SESSION_TESTS`]), for a developer's run, CI's, and the
//! compositor tests' bundle ([`crate::compositor_tests`]), which carries
//! them to CI's shards built.
//!
//! Each binary runs its ignored tests (the ones that need a compositor),
//! told the compositor in [`COMPOSITOR_VARIABLE`], and every one runs
//! whatever fails before it, so that one run reports every failure.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Outcome;
use crate::compositor_tests::{Backend, Target, test_binaries};
use crate::error::Error;
use crate::session::Compositor;

/// The variable the tests read their compositor from.
pub(crate) const COMPOSITOR_VARIABLE: &str = "DESKRINCH_SESSION_TEST_COMPOSITOR";

/// One of xtask's `[[test]]` targets run against a compositor, and the
/// libtest arguments it runs with beyond `--ignored`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionTest {
    /// The `[[test]]` name.
    pub(crate) name: &'static str,
    /// Its extra arguments.
    args: &'static [&'static str],
}

/// The tests, in the order they run. The scheduler's figures come first,
/// their output shown (`--nocapture`): a stand-in's, the compositor's and
/// the bus's schedstat and thread states, idle and starved, printed for
/// each runner.
pub(crate) const SESSION_TESTS: [SessionTest; 13] = [
    SessionTest {
        name: "quiet_figures",
        args: &["--nocapture"],
    },
    plain("measure"),
    plain("measure_busy_compositor"),
    plain("measure_callgrind"),
    plain("measure_command_steps"),
    plain("measure_pointer"),
    plain("measure_prepares"),
    plain("measure_quiet"),
    plain("measure_start_ends"),
    plain("measure_typing"),
    plain("measure_windows"),
    plain("session"),
    plain("session_sizes"),
];

/// A test that runs with `--ignored` alone.
const fn plain(name: &'static str) -> SessionTest {
    SessionTest { name, args: &[] }
}

impl SessionTest {
    /// Its target, built as the compositor tests build theirs.
    pub(crate) fn target(self) -> Target {
        Target {
            package: "xtask",
            test: self.name,
            backend: Backend::Rust,
        }
    }
}

/// `cargo xtask session-tests`'s command line.
#[derive(Debug, Clone, PartialEq, Eq, clap::Args)]
pub(crate) struct Args {
    /// Run the binaries of the compositor tests' bundle
    /// (`target/compositor-bundle`, unpacked from the archive `cargo xtask
    /// bundle build` makes) instead of building them.
    #[arg(long)]
    pub(crate) prebuilt: bool,
    /// The compositor the tests start their sessions with.
    #[arg(value_enum)]
    pub(crate) compositor: Compositor,
}

/// Builds every one of [`SESSION_TESTS`] in one `cargo test --no-run`; each
/// test's name and binary.
pub(crate) fn build(root: &Path) -> Result<Vec<(&'static str, PathBuf)>, Error> {
    let targets: Vec<Target> = SESSION_TESTS.iter().map(|test| test.target()).collect();
    let built = test_binaries(root, &targets)?;
    Ok(SESSION_TESTS
        .iter()
        .zip(built)
        .map(|(test, binaries)| (test.name, binaries.test))
        .collect())
}

/// Runs the tests as `args` asks, from the workspace at `root`.
pub(crate) fn run(root: &Path, args: &Args) -> Result<Outcome, Error> {
    let binaries = if args.prebuilt {
        crate::compositor_tests::Bundle::open(root)?.session_tests()?
    } else {
        build(root)?
    };
    let mut failed = Vec::new();
    for test in SESSION_TESTS {
        let binary = binaries
            .iter()
            .find(|(name, _)| *name == test.name)
            .map(|(_, binary)| binary)
            .ok_or(Error::MissingTestBinary {
                package: "xtask",
                name: test.name,
            })?;
        println!(
            "session-tests: {} under {}",
            test.name,
            args.compositor.name()
        );
        let status = command(root, binary, test, args.compositor)
            .status()
            .map_err(|source| Error::Spawn {
                program: binary.display().to_string(),
                source,
            })?;
        if !status.success() {
            failed.push(test.name);
        }
    }
    if failed.is_empty() {
        println!("session-tests: passed");
        Ok(Outcome::Passed)
    } else {
        eprintln!("session-tests: failed: {}", failed.join(", "));
        Ok(Outcome::Failed)
    }
}

/// The command that runs `test`'s `binary` under `compositor`, from xtask's
/// directory as `cargo test` runs it.
fn command(root: &Path, binary: &Path, test: SessionTest, compositor: Compositor) -> Command {
    let mut command = Command::new(binary);
    command
        .current_dir(root.join("xtask"))
        .arg("--ignored")
        .args(test.args)
        .env(COMPOSITOR_VARIABLE, compositor.name());
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_session_test_is_one_of_xtask_s_targets_and_listed_once() {
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        for (index, test) in SESSION_TESTS.iter().enumerate() {
            let file = tests.join(format!("{}.rs", test.name));
            assert!(file.exists(), "{} is no test of xtask's", test.name);
            assert!(
                SESSION_TESTS[index + 1..]
                    .iter()
                    .all(|later| later.name != test.name),
                "{} twice",
                test.name
            );
        }
    }

    #[test]
    fn a_test_runs_its_ignored_tests_told_the_compositor() {
        let root = Path::new("/w");
        let figures = command(
            root,
            Path::new("/b/quiet_figures"),
            SESSION_TESTS[0],
            Compositor::Sway,
        );
        let args: Vec<_> = figures.get_args().collect();
        assert_eq!(args, ["--ignored", "--nocapture"]);
        assert_eq!(figures.get_current_dir(), Some(Path::new("/w/xtask")));
        let env: Vec<_> = figures.get_envs().collect();
        assert_eq!(
            env,
            [(
                std::ffi::OsStr::new(COMPOSITOR_VARIABLE),
                Some(std::ffi::OsStr::new("sway"))
            )]
        );
        let session = command(
            root,
            Path::new("/b/session"),
            plain("session"),
            Compositor::Kwin,
        );
        assert_eq!(session.get_args().collect::<Vec<_>>(), ["--ignored"]);
    }
}

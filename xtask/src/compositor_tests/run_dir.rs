//! The run's own copy of each component it built, so that what the suites
//! start does not depend on what builds after the run's build.
//!
//! Cargo puts a component's binary at one path in the target directory (the
//! uplifted `target/debug/deskrinch-panel`), and every build of it there
//! writes that path: a test build (the binary built with the test crates'
//! features), and any `cargo` the owner runs in the checkout meanwhile. A
//! run itself does: the desktop suite's tests run `cargo xtask desktop`,
//! which builds the components again. So the runner keeps each built
//! component in a directory of the run's own, `compositor-tests/run-<pid>`
//! in the target directory, and hands the suites those paths: a hard link,
//! which Cargo leaves alone when it replaces the uplifted file (it removes
//! it and makes a new one), or a copy where the link is refused. Which of
//! the two was made matters to nobody: either way the suites get a path that
//! nothing else writes.
//!
//! The directory goes when the run ends. A run that was killed leaves its
//! own, holding full binaries once Cargo has replaced the uplifted files,
//! so a run first removes those whose process is gone ([`is_stale`]), and
//! never one whose process runs.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rustix::process::{Pid, test_kill_process};

/// What the run directories are named, before the run's process id.
const PREFIX: &str = "run-";

/// The run's directory of components; removed when dropped.
#[derive(Debug)]
pub(super) struct RunDir {
    path: PathBuf,
}

impl RunDir {
    /// Makes this run's directory beside the target directory holding
    /// `built` (a binary Cargo put at `<target>/<profile>/<name>`), after
    /// removing the directories of runs whose process is gone.
    pub(super) fn beside(built: &Path) -> io::Result<Self> {
        let target = built.parent().and_then(Path::parent).ok_or_else(|| {
            io::Error::other(format!("no target directory above {}", built.display()))
        })?;
        let parent = target.join("compositor-tests");
        remove_stale(&parent, process_runs);
        let path = parent.join(format!("{PREFIX}{}", std::process::id()));
        // A directory left by an earlier process of this id is the run's to
        // replace.
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    /// Keeps `built` in the run's directory, under its own name; its path
    /// there.
    pub(super) fn keep(&self, built: &Path) -> io::Result<PathBuf> {
        let name = built
            .file_name()
            .ok_or_else(|| io::Error::other(format!("{} names no file", built.display())))?;
        let kept = self.path.join(name);
        if fs::hard_link(built, &kept).is_err() {
            fs::copy(built, &kept)?;
        }
        Ok(kept)
    }
}

impl Drop for RunDir {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            eprintln!(
                "compositor-tests: could not remove {}: {error}",
                self.path.display()
            );
        }
    }
}

/// Whether process `pid` runs (or exists, as a zombie or another user's).
fn process_runs(pid: i32) -> bool {
    let Some(pid) = Pid::from_raw(pid) else {
        return false;
    };
    match test_kill_process(pid) {
        Ok(()) => true,
        // It exists, and is not ours to signal.
        Err(error) => error == rustix::io::Errno::PERM,
    }
}

/// Removes the run directories in `parent` whose process is gone, as
/// `runs` says; leaves everything else.
fn remove_stale(parent: &Path, runs: impl Fn(i32) -> bool) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        if is_stale(&name.to_string_lossy(), &runs) {
            // Another run may be removing it too.
            drop(fs::remove_dir_all(entry.path()));
        }
    }
}

/// Whether the directory named `name` is a run directory whose process is
/// gone, as `runs` says of a process id: `run-<pid>`, `pid` a positive
/// number, and `runs(pid)` false. Anything else, and the directory of a
/// process that runs, is not.
fn is_stale(name: &str, runs: impl Fn(i32) -> bool) -> bool {
    name.strip_prefix(PREFIX)
        .filter(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|pid| pid.parse::<i32>().ok())
        .is_some_and(|pid| pid > 0 && !runs(pid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_directory_whose_process_is_gone_is_stale() {
        let gone = |_| false;
        assert!(is_stale("run-4242", gone));
    }

    #[test]
    fn the_directory_of_a_process_that_runs_is_never_stale() {
        let runs = |pid| pid == 4242;
        assert!(!is_stale("run-4242", runs));
        assert!(is_stale("run-4243", runs));
    }

    #[test]
    fn anything_but_a_run_directory_is_never_stale() {
        let gone = |_| false;
        for name in [
            "run-",
            "run-0",
            "run--1",
            "run-12a",
            "run-+12",
            "keep",
            "run-99999999999",
        ] {
            assert!(!is_stale(name, gone), "{name}");
        }
    }

    #[test]
    fn stale_directories_go_and_the_rest_stay() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["run-1", "run-2", "other"] {
            fs::create_dir(dir.path().join(name)).unwrap();
            fs::write(dir.path().join(name).join("deskrinch-panel"), b"x").unwrap();
        }
        remove_stale(dir.path(), |pid| pid == 2);
        assert!(!dir.path().join("run-1").exists());
        assert!(dir.path().join("run-2").exists());
        assert!(dir.path().join("other").exists());
    }

    #[test]
    fn this_process_runs() {
        assert!(process_runs(i32::try_from(std::process::id()).unwrap()));
    }

    #[test]
    fn a_kept_component_does_not_change_when_the_built_one_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let built = dir.path().join("debug").join("deskrinch-panel");
        fs::create_dir_all(built.parent().unwrap()).unwrap();
        fs::write(&built, b"first").unwrap();
        let run = RunDir::beside(&built).unwrap();
        let kept = run.keep(&built).unwrap();
        // As Cargo replaces an uplifted file: removed, then made anew.
        fs::remove_file(&built).unwrap();
        fs::write(&built, b"second").unwrap();
        assert_eq!(fs::read(&kept).unwrap(), b"first");
        let path = kept.parent().unwrap().to_owned();
        drop(run);
        assert!(!path.exists(), "the run's directory goes with it");
    }
}

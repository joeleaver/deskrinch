//! Temporary directories named for the process that owns them, and the
//! sweep of those whose owner has gone.
//!
//! A directory is named `deskrinch-test-<pid>-<start>-<random>`, `<start>`
//! being the owner's start time (in clock ticks since boot, from
//! `/proc/<pid>/stat`): a pid alone is reused, a pid and its start time
//! name one process for good.

use std::fs;
use std::io;
use std::path::Path;

use rustix::process::{Pid, getpid};
use tempfile::TempDir;

/// What every owned directory's name starts with.
const PREFIX: &str = "deskrinch-test-";

/// A new temporary directory, removed when dropped, named for this process;
/// first, the directories of processes that have gone are removed, so that
/// one left by a killed test lasts at most until the next.
///
/// # Errors
///
/// When this process's start time cannot be read, or the directory cannot
/// be made.
pub fn owned_dir() -> io::Result<TempDir> {
    let pid = getpid();
    let start = start_time(pid).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "this process's start time is not in /proc",
        )
    })?;
    sweep(&std::env::temp_dir());
    tempfile::Builder::new()
        .prefix(&format!("{PREFIX}{}-{start}-", pid.as_raw_nonzero()))
        .tempdir()
}

/// Removes the owned directories in `root` whose owner has gone. One that
/// cannot be removed (another user's, or one being removed by another
/// process at the same time) is left.
fn sweep(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some((pid, start)) = name.to_str().and_then(owner) else {
            continue;
        };
        if start_time(pid) != Some(start) {
            let _removed = fs::remove_dir_all(entry.path());
        }
    }
}

/// The owner a directory's name gives: its pid and start time.
fn owner(name: &str) -> Option<(Pid, u64)> {
    let mut fields = name.strip_prefix(PREFIX)?.splitn(3, '-');
    let pid = Pid::from_raw(fields.next()?.parse().ok()?)?;
    let start = fields.next()?.parse().ok()?;
    fields.next()?;
    Some((pid, start))
}

/// The start time of the process `pid`, in clock ticks since boot; `None`
/// when there is no such process.
fn start_time(pid: Pid) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{}/stat", pid.as_raw_nonzero())).ok()?;
    // The command name, in parentheses, may hold spaces and parentheses:
    // the fields are counted from the last `)`. The start time is field 22,
    // the 20th after it.
    let (_, fields) = stat.rsplit_once(')')?;
    fields.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_names_its_owner() {
        let dir = owned_dir().unwrap();
        let name = dir.path().file_name().unwrap().to_str().unwrap();
        let (pid, start) = owner(name).unwrap();
        assert_eq!(pid, getpid());
        assert_eq!(Some(start), start_time(pid));
    }

    #[test]
    fn the_sweep_removes_only_the_directories_of_processes_gone() {
        let root = tempfile::tempdir().unwrap();
        let me = getpid();
        let alive = format!(
            "{PREFIX}{}-{}-x",
            me.as_raw_nonzero(),
            start_time(me).unwrap()
        );
        // This process's pid with another start time: a process that had
        // the pid before, gone.
        let reused = format!(
            "{PREFIX}{}-{}-x",
            me.as_raw_nonzero(),
            start_time(me).unwrap() + 1
        );
        let unrelated = "deskrinch-test-not-an-owner";
        for name in [&alive, &reused, unrelated] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        sweep(root.path());
        let mut left: Vec<_> = fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        let mut expected = vec![alive, unrelated.to_owned()];
        expected.sort();
        assert_eq!(left, expected);
    }

    #[test]
    fn names_that_are_not_owners_are_not_read_as_ones() {
        for name in [
            "deskrinch-test-",
            "deskrinch-test-12",
            "deskrinch-test-12-x-y",
            "deskrinch-test-0-1-x",
            "other-12-34-x",
        ] {
            assert_eq!(owner(name), None, "{name}");
        }
        assert_eq!(
            owner("deskrinch-test-12-34-abc"),
            Some((Pid::from_raw(12).unwrap(), 34))
        );
    }
}

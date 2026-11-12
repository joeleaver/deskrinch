//! Temporary directories named for the process that owns them, and the
//! sweep of those whose owner has gone.
//!
//! A directory is named `deskrinch-test-<namespace>-<pid>-<start>-<random>`:
//! the owner's pid namespace (the inode `/proc/self/ns/pid` names), its pid
//! there, and its start time (in clock ticks since boot, from
//! `/proc/<pid>/stat`). A pid alone is reused; a pid and its start time
//! name one process for good, but only within its own pid namespace: two
//! namespaces may share `/tmp` (a sandbox, a container with it mounted in),
//! and a pid of the other means nothing here. The sweep judges only the
//! directories of its own namespace.

use std::fs;
use std::io;
use std::path::Path;

use rustix::process::{Pid, getpid};
use tempfile::TempDir;

/// What every owned directory's name starts with.
const PREFIX: &str = "deskrinch-test-";

/// A new temporary directory, removed when dropped, named for this process;
/// first, the directories of processes of this pid namespace that have gone
/// are removed, so that one left by a killed test lasts at most until the
/// next.
///
/// # Errors
///
/// When this process's pid namespace or start time cannot be read, or the
/// directory cannot be made.
pub fn owned_dir() -> io::Result<TempDir> {
    let me = Owner::this()?;
    sweep(&std::env::temp_dir(), me.namespace);
    tempfile::Builder::new()
        .prefix(&format!(
            "{PREFIX}{}-{}-{}-",
            me.namespace,
            me.pid.as_raw_nonzero(),
            me.start
        ))
        .tempdir()
}

/// A process, named for good: its pid namespace, its pid there, and its
/// start time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Owner {
    namespace: u64,
    pid: Pid,
    start: u64,
}

impl Owner {
    /// This process.
    fn this() -> io::Result<Self> {
        let missing = |what| io::Error::new(io::ErrorKind::NotFound, what);
        let pid = getpid();
        Ok(Self {
            namespace: namespace().ok_or_else(|| missing("this process's pid namespace"))?,
            pid,
            start: start_time(pid).ok_or_else(|| missing("this process's start time"))?,
        })
    }

    /// The owner a directory's name gives.
    fn of(name: &str) -> Option<Self> {
        let mut fields = name.strip_prefix(PREFIX)?.splitn(4, '-');
        let namespace = fields.next()?.parse().ok()?;
        let pid = Pid::from_raw(fields.next()?.parse().ok()?)?;
        let start = fields.next()?.parse().ok()?;
        fields.next()?;
        Some(Self {
            namespace,
            pid,
            start,
        })
    }

    /// Whether this process (of this namespace) still runs: its pid names
    /// a process that started when it did.
    fn running(self) -> bool {
        start_time(self.pid) == Some(self.start)
    }
}

/// Removes the owned directories in `root` of processes of the pid
/// namespace `namespace` that have gone. Those of any other namespace are
/// left: their pids mean nothing here. One that cannot be removed (another
/// user's, or one being removed by another process at the same time) is
/// left too.
fn sweep(root: &Path, namespace: u64) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(owner) = name.to_str().and_then(Owner::of) else {
            continue;
        };
        if owner.namespace == namespace && !owner.running() {
            let _removed = fs::remove_dir_all(entry.path());
        }
    }
}

/// This process's pid namespace: the inode `/proc/self/ns/pid` names, as
/// `pid:[<inode>]`.
fn namespace() -> Option<u64> {
    let link = fs::read_link("/proc/self/ns/pid").ok()?;
    link.to_str()?
        .strip_prefix("pid:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
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

    fn name(owner: Owner) -> String {
        format!(
            "{PREFIX}{}-{}-{}-x",
            owner.namespace,
            owner.pid.as_raw_nonzero(),
            owner.start
        )
    }

    #[test]
    fn a_directory_names_its_owner() {
        let dir = owned_dir().unwrap();
        let name = dir.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(Owner::of(name), Some(Owner::this().unwrap()));
    }

    #[test]
    fn the_sweep_removes_only_this_namespace_s_directories_of_processes_gone() {
        let root = tempfile::tempdir().unwrap();
        let me = Owner::this().unwrap();
        // This process's pid with another start time: a process that had
        // the pid before, gone.
        let gone = Owner {
            start: me.start + 1,
            ..me
        };
        // A process of another namespace, whatever its pid: its pid names
        // nothing here, gone or not.
        let elsewhere = Owner {
            namespace: me.namespace + 1,
            ..gone
        };
        let unrelated = "deskrinch-test-not-an-owner".to_owned();
        for name in [name(me), name(gone), name(elsewhere), unrelated.clone()] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        sweep(root.path(), me.namespace);
        let mut left: Vec<_> = fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        let mut expected = vec![name(me), name(elsewhere), unrelated];
        expected.sort();
        assert_eq!(left, expected);
    }

    #[test]
    fn names_that_are_not_owners_are_not_read_as_ones() {
        for name in [
            "deskrinch-test-",
            "deskrinch-test-1-12",
            "deskrinch-test-1-12-34",
            "deskrinch-test-1-12-x-y",
            "deskrinch-test-1-0-34-x",
            "deskrinch-test-x-12-34-x",
            "other-1-12-34-x",
        ] {
            assert_eq!(Owner::of(name), None, "{name}");
        }
        assert_eq!(
            Owner::of("deskrinch-test-7-12-34-abc"),
            Some(Owner {
                namespace: 7,
                pid: Pid::from_raw(12).unwrap(),
                start: 34,
            })
        );
    }
}

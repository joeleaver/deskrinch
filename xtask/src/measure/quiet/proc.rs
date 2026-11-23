//! What the quiet window reads of a process from `/proc`: each thread's
//! run time and run-queue wait (`schedstat`) and its state (`stat`), and
//! the process table's parent links, for a process's descendants.
//!
//! The parsers are pure functions of the files' text, tested without a
//! process; [`read_tasks`] and [`read_parents`] only fetch the text.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// One thread at one poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Task {
    /// CPU time it has run, in nanoseconds (`schedstat`'s first field).
    pub(crate) run_ns: u64,
    /// Time it has waited on a run queue, runnable but not running, in
    /// nanoseconds (`schedstat`'s second field). The kernel adds a wait
    /// when the thread next gets the CPU, so a thread starved through a
    /// whole window shows none of it there: its state says it instead.
    pub(crate) wait_ns: u64,
    /// Its state letter (`stat`'s third field): `R` running or runnable,
    /// `D` in uninterruptible sleep (disk), `S` sleeping, and so on.
    pub(crate) state: char,
}

impl Task {
    /// Whether it was running, waiting to run, or blocked on I/O at the
    /// poll: anything but asleep or stopped.
    pub(crate) fn busy(self) -> bool {
        matches!(self.state, 'R' | 'D')
    }
}

/// Every live thread of `pid`, by thread id; empty once the process has
/// gone. A thread that ends while it is read is left out.
pub(crate) fn read_tasks(pid: u32) -> BTreeMap<u32, Task> {
    let dir = Path::new("/proc").join(pid.to_string()).join("task");
    let Ok(entries) = fs::read_dir(&dir) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let tid = entry.file_name().to_str()?.parse().ok()?;
            let path = entry.path();
            let schedstat = fs::read_to_string(path.join("schedstat")).ok()?;
            let stat = fs::read_to_string(path.join("stat")).ok()?;
            let (run_ns, wait_ns) = parse_schedstat(&schedstat)?;
            let (state, _) = parse_stat(&stat)?;
            Some((
                tid,
                Task {
                    run_ns,
                    wait_ns,
                    state,
                },
            ))
        })
        .collect()
}

/// The children of `pid`: every one of its threads' `children` file
/// (`/proc/<pid>/task/<tid>/children`), which lists the processes that
/// thread started. Reading them from the compositor down costs what its
/// few descendants cost, where a scan of the whole process table costs
/// what every process on the machine does (28 ms a poll over a thousand
/// processes, measured at a load of 109).
pub(crate) fn read_children(pid: u32) -> Vec<u32> {
    let dir = Path::new("/proc").join(pid.to_string()).join("task");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path().join("children")).ok())
        .flat_map(|text| parse_children(&text))
        .collect()
}

/// How a process's children are found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Children {
    /// The threads' `children` files ([`read_children`]).
    Files,
    /// A kernel built without them (`CONFIG_PROC_CHILDREN` off): the whole
    /// process table's parent links, read at every poll. Slower, never
    /// blind: descendants are not quietly left unwatched.
    Scan,
}

impl Children {
    /// The files when this kernel has them, the scan when it does not.
    pub(crate) fn detect() -> Self {
        Self::of(Path::new("/proc/thread-self/children").exists())
    }

    /// The files if `files_exist`, else the scan.
    pub(crate) fn of(files_exist: bool) -> Self {
        if files_exist { Self::Files } else { Self::Scan }
    }

    /// Every descendant of `root`, found this way.
    pub(crate) fn descendants(self, root: u32) -> BTreeSet<u32> {
        match self {
            Self::Files => descendants(read_children, root),
            Self::Scan => {
                let by_parent = children_by_parent(&read_parents());
                descendants(|pid| by_parent.get(&pid).cloned().unwrap_or_default(), root)
            }
        }
    }
}

/// Each process's parent, by process id, from the whole process table.
fn read_parents() -> BTreeMap<u32, u32> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return BTreeMap::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
            let (_, parent) = parse_stat(&stat)?;
            Some((pid, parent))
        })
        .collect()
}

/// Each process's children, from each process's parent.
pub(crate) fn children_by_parent(parents: &BTreeMap<u32, u32>) -> BTreeMap<u32, Vec<u32>> {
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (&child, &parent) in parents {
        children.entry(parent).or_default().push(child);
    }
    children
}

/// The process ids in a `children` file: separated by spaces.
pub(crate) fn parse_children(children: &str) -> Vec<u32> {
    children
        .split_whitespace()
        .filter_map(|pid| pid.parse().ok())
        .collect()
}

/// Run time and run-queue wait, in nanoseconds, from a `schedstat` line.
pub(crate) fn parse_schedstat(schedstat: &str) -> Option<(u64, u64)> {
    let mut fields = schedstat.split_whitespace();
    let run = fields.next()?.parse().ok()?;
    let wait = fields.next()?.parse().ok()?;
    Some((run, wait))
}

/// State and parent id from a `stat` line. The command name in
/// parentheses may itself hold spaces and parentheses, so fields are
/// counted from the last `)`.
pub(crate) fn parse_stat(stat: &str) -> Option<(char, u32)> {
    let mut fields = stat[stat.rfind(')')? + 1..].split_whitespace();
    let state = fields.next()?.chars().next()?;
    let parent = fields.next()?.parse().ok()?;
    Some((state, parent))
}

/// Every descendant of `root`, at any depth, `children` giving a process's
/// children; not `root` itself. A process seen twice (a listing taken
/// while processes come and go) is counted once.
pub(crate) fn descendants(children: impl Fn(u32) -> Vec<u32>, root: u32) -> BTreeSet<u32> {
    let mut found = BTreeSet::new();
    let mut next = vec![root];
    while let Some(pid) = next.pop() {
        for child in children(pid) {
            if child != root && found.insert(child) {
                next.push(child);
            }
        }
    }
    found
}

//! What `/proc` says about a process and its threads: CPU time, context
//! switches and memory, parsed from the kernel's text formats.
//!
//! Every reader here is a pure function of the file's text, so the parsing
//! is tested without a process; [`read_process`] and [`read_threads`] only
//! fetch the text.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// One thread at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThreadSample {
    /// Its name (`comm`), as the kernel truncates it (15 bytes).
    pub(crate) name: String,
    /// CPU time it has run, in nanoseconds (`schedstat`'s first field).
    pub(crate) runtime_ns: u64,
    /// Times it gave up the CPU to wait: each one ends in a wake-up.
    pub(crate) voluntary_switches: u64,
    /// Times it was preempted.
    pub(crate) involuntary_switches: u64,
}

/// The whole process at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProcessSample {
    /// User CPU time of every thread, exited ones included, in clock ticks.
    pub(crate) user_ticks: u64,
    /// System CPU time likewise.
    pub(crate) system_ticks: u64,
    /// The `kB` fields of `status` this task reports (`VmRSS`, `RssAnon`,
    /// `RssFile`, `RssShmem`, `VmSwap`) and `Threads`.
    pub(crate) status: BTreeMap<String, u64>,
}

/// The fields of `status` that are kept.
pub(crate) const STATUS_FIELDS: [&str; 6] = [
    "VmRSS", "RssAnon", "RssFile", "RssShmem", "VmSwap", "Threads",
];

/// Reads the process `pid`, or `None` once it has gone.
pub(crate) fn read_process(pid: u32) -> Option<ProcessSample> {
    let dir = Path::new("/proc").join(pid.to_string());
    let stat = fs::read_to_string(dir.join("stat")).ok()?;
    let status = fs::read_to_string(dir.join("status")).ok()?;
    let (user_ticks, system_ticks) = parse_stat_times(&stat)?;
    Some(ProcessSample {
        user_ticks,
        system_ticks,
        status: parse_status(&status),
    })
}

/// Reads every live thread of `pid`, by thread id. Threads that end while
/// they are read are left out.
pub(crate) fn read_threads(pid: u32) -> BTreeMap<u32, ThreadSample> {
    let dir = Path::new("/proc").join(pid.to_string()).join("task");
    let Ok(entries) = fs::read_dir(&dir) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let tid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let path = entry.path();
            let name = fs::read_to_string(path.join("comm")).ok()?;
            let schedstat = fs::read_to_string(path.join("schedstat")).ok()?;
            let status = fs::read_to_string(path.join("status")).ok()?;
            let switches = parse_status(&status);
            Some((
                tid,
                ThreadSample {
                    name: name.trim_end().to_owned(),
                    runtime_ns: parse_schedstat(&schedstat)?,
                    voluntary_switches: switches.get("voluntary_ctxt_switches").copied()?,
                    involuntary_switches: switches.get("nonvoluntary_ctxt_switches").copied()?,
                },
            ))
        })
        .collect()
}

/// `utime` and `stime` from `/proc/<pid>/stat`. The command name in
/// parentheses may itself hold spaces and parentheses, so fields are
/// counted from the last `)`.
pub(crate) fn parse_stat_times(stat: &str) -> Option<(u64, u64)> {
    let rest = &stat[stat.rfind(')')? + 1..];
    // After the name: state is field 3, utime field 14, stime field 15.
    let mut fields = rest.split_whitespace().skip(11);
    let user = fields.next()?.parse().ok()?;
    let system = fields.next()?.parse().ok()?;
    Some((user, system))
}

/// The run time, in nanoseconds, from a `schedstat` line.
pub(crate) fn parse_schedstat(schedstat: &str) -> Option<u64> {
    schedstat.split_whitespace().next()?.parse().ok()
}

/// The numeric fields of a `status` file: `Name:\t1234 kB` and
/// `Name:\t1234` both give `Name -> 1234`.
pub(crate) fn parse_status(status: &str) -> BTreeMap<String, u64> {
    status
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let number = value.split_whitespace().next()?.parse().ok()?;
            Some((key.trim().to_owned(), number))
        })
        .collect()
}

/// A thread's name with a trailing number taken off, so that a pool of
/// workers (`llvmpipe-12`, `deskrin:disk$3`) adds up as one group.
pub(crate) fn thread_group(name: &str) -> String {
    let trimmed = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.len() == name.len() || trimmed.is_empty() {
        name.to_owned()
    } else {
        format!("{trimmed}N")
    }
}

/// Run time added up by [`thread_group`]: `(group, total ns, threads)`, the
/// largest first.
pub(crate) fn runtime_by_group<'a>(
    threads: impl IntoIterator<Item = &'a ThreadSample>,
) -> Vec<(String, u64, u32)> {
    by_group(
        threads
            .into_iter()
            .map(|thread| (thread.name.as_str(), thread.runtime_ns)),
    )
}

/// The wake-ups (voluntary switches) between `before` and `after`, both by
/// thread id, added up by [`thread_group`]: `(group, wake-ups, threads)`,
/// the most first. A thread that started in between counts from zero; one
/// that ended in between is not seen.
pub(crate) fn wake_ups_by_group(
    before: &BTreeMap<u32, ThreadSample>,
    after: &BTreeMap<u32, ThreadSample>,
) -> Vec<(String, u64, u32)> {
    by_group(after.iter().map(|(tid, now)| {
        let was = before.get(tid).map_or(0, |then| then.voluntary_switches);
        (
            now.name.as_str(),
            now.voluntary_switches.saturating_sub(was),
        )
    }))
}

/// `(name, amount)` pairs added up by [`thread_group`]: `(group, total,
/// threads)`, the largest first, then by name.
fn by_group<'a>(threads: impl IntoIterator<Item = (&'a str, u64)>) -> Vec<(String, u64, u32)> {
    let mut groups: BTreeMap<String, (u64, u32)> = BTreeMap::new();
    for (name, amount) in threads {
        let entry = groups.entry(thread_group(name)).or_default();
        entry.0 += amount;
        entry.1 += 1;
    }
    let mut groups: Vec<_> = groups
        .into_iter()
        .map(|(name, (ns, count))| (name, ns, count))
        .collect();
    groups.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_times_skip_a_name_with_spaces_and_parentheses() {
        let stat = "4242 (Vello (shader) in) S 1 4242 4242 0 -1 4194560 2263 0 0 0 \
                    76 9 0 0 20 0 10 0 123 456 789";
        assert_eq!(parse_stat_times(stat), Some((76, 9)));
        assert_eq!(parse_stat_times("4242 (cut"), None);
        assert_eq!(parse_stat_times("4242 (short) S 1 2"), None);
    }

    #[test]
    fn schedstat_gives_the_run_time() {
        assert_eq!(parse_schedstat("579123456 2000 31\n"), Some(579_123_456));
        assert_eq!(parse_schedstat(""), None);
    }

    #[test]
    fn status_fields_are_numbers_with_or_without_a_unit() {
        let status = "Name:\tdeskrinch-panel\nThreads:\t4\nVmRSS:\t  109996 kB\n\
                      RssShmem:\t       4 kB\nvoluntary_ctxt_switches:\t15\n";
        let fields = parse_status(status);
        assert_eq!(fields.get("VmRSS"), Some(&109_996));
        assert_eq!(fields.get("RssShmem"), Some(&4));
        assert_eq!(fields.get("Threads"), Some(&4));
        assert_eq!(fields.get("voluntary_ctxt_switches"), Some(&15));
        assert_eq!(fields.get("Name"), None, "not a number");
    }

    #[test]
    fn worker_pools_group_by_name() {
        assert_eq!(thread_group("llvmpipe-12"), "llvmpipe-N");
        assert_eq!(thread_group("deskrin:disk$3"), "deskrin:disk$N");
        assert_eq!(thread_group("clock"), "clock");
        assert_eq!(thread_group("42"), "42", "a name that is only digits stays");
    }

    #[test]
    fn wake_ups_are_each_thread_s_own_since_before() {
        let thread = |name: &str, voluntary_switches| ThreadSample {
            name: name.to_owned(),
            runtime_ns: 0,
            voluntary_switches,
            involuntary_switches: 0,
        };
        let before = BTreeMap::from([
            (1, thread("panel", 100)),
            (2, thread("worker-1", 10)),
            (3, thread("gone", 5)),
        ]);
        let after = BTreeMap::from([
            (1, thread("panel", 104)),
            (2, thread("worker-1", 640)),
            // Started inside the window: from zero.
            (4, thread("worker-2", 20)),
            (5, thread("clock", 0)),
        ]);
        assert_eq!(
            wake_ups_by_group(&before, &after),
            [
                ("worker-N".to_owned(), 650, 2),
                ("panel".to_owned(), 4, 1),
                ("clock".to_owned(), 0, 1),
            ]
        );
    }

    #[test]
    fn groups_add_up_and_sort_largest_first() {
        let thread = |name: &str, runtime_ns| ThreadSample {
            name: name.to_owned(),
            runtime_ns,
            voluntary_switches: 0,
            involuntary_switches: 0,
        };
        let threads = [
            thread("main", 250),
            thread("worker-1", 300),
            thread("worker-2", 279),
            thread("clock", 0),
        ];
        assert_eq!(
            runtime_by_group(&threads),
            [
                ("worker-N".to_owned(), 579, 2),
                ("main".to_owned(), 250, 1),
                ("clock".to_owned(), 0, 1),
            ]
        );
    }
}

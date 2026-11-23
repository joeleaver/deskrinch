//! One quiet window's tally: what each watched process's threads ran and
//! waited between the window's start and its end, how many polls found one
//! of them busy, and whether the polls themselves came on time. A pure
//! accumulation over polls, tested without a process.

use std::collections::BTreeMap;
use std::time::Duration;

use super::super::probe::nanos;
use super::proc::Task;
use super::{MAX_POLL_GAP, MIN_POLLS, QUIET_RUN, QUIET_WAIT};

/// One poll of the watched processes: each by its label, with its threads.
pub(crate) type Polled = Vec<(String, BTreeMap<u32, Task>)>;

/// The window so far.
#[derive(Debug, Clone)]
pub(crate) struct Tally {
    members: BTreeMap<String, Member>,
    /// When each poll was made, from the window's start.
    polls: Vec<Duration>,
}

/// One watched process's part of the window.
#[derive(Debug, Clone, Default)]
struct Member {
    /// Each thread's run and wait at its last poll.
    previous: BTreeMap<u32, (u64, u64)>,
    run_ns: u64,
    wait_ns: u64,
    busy_polls: u32,
}

/// What a window held for one watched process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Figures {
    /// Which process ("the program", "the compositor", ...).
    pub(crate) label: String,
    /// Its threads' run time in the window, summed.
    pub(crate) run_ns: u64,
    /// Its threads' run-queue wait in the window, summed.
    pub(crate) wait_ns: u64,
    /// Polls at which one of its threads was running, runnable or blocked
    /// on I/O (`R` or `D`).
    pub(crate) busy_polls: u32,
}

impl Figures {
    /// Whether this process was quiet in the window.
    pub(crate) fn quiet(&self) -> bool {
        self.run_ns < nanos(QUIET_RUN) && self.wait_ns < nanos(QUIET_WAIT) && self.busy_polls == 0
    }
}

/// What a whole window held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verdict {
    /// Whether the tool made its polls on time: at least [`MIN_POLLS`], none
    /// more than [`MAX_POLL_GAP`] after the one before it (or the window's
    /// start), and the window's end no more than that after the last. A
    /// window the tool was itself starved through looked too seldom to be
    /// called quiet.
    pub(crate) on_time: bool,
    /// Each watched process's figures, by its label.
    pub(crate) figures: Vec<Figures>,
}

impl Verdict {
    /// Whether the window was quiet: polled on time, and every watched
    /// process quiet in it.
    pub(crate) fn quiet(&self) -> bool {
        self.on_time && self.figures.iter().all(Figures::quiet)
    }
}

impl Tally {
    /// A window starting from `start`, a poll whose figures are the
    /// baseline and are not counted.
    pub(crate) fn new(start: &Polled) -> Self {
        let members = start
            .iter()
            .map(|(label, tasks)| {
                let member = Member {
                    previous: tasks
                        .iter()
                        .map(|(&tid, task)| (tid, (task.run_ns, task.wait_ns)))
                        .collect(),
                    ..Member::default()
                };
                (label.clone(), member)
            })
            .collect();
        Self {
            members,
            polls: Vec::new(),
        }
    }

    /// Adds a poll made `at` after the window's start. A thread first seen
    /// here counts all it ran and waited (both start at zero when it is
    /// born); a process first seen here (a child the compositor has just
    /// started) likewise. A thread gone since the last poll keeps what its
    /// earlier polls counted; only its last moments are lost.
    pub(crate) fn add(&mut self, at: Duration, polled: &Polled) {
        self.polls.push(at);
        for (label, tasks) in polled {
            let member = self.members.entry(label.clone()).or_default();
            let mut busy = false;
            for (&tid, task) in tasks {
                let (run, wait) = member.previous.get(&tid).copied().unwrap_or((0, 0));
                member.run_ns += task.run_ns.saturating_sub(run);
                member.wait_ns += task.wait_ns.saturating_sub(wait);
                member.previous.insert(tid, (task.run_ns, task.wait_ns));
                busy |= task.busy();
            }
            member.busy_polls += u32::from(busy);
        }
    }

    /// The verdict on a window of length `window`.
    pub(crate) fn verdict(&self, window: Duration) -> Verdict {
        let mut last = Duration::ZERO;
        let mut gaps_ok = true;
        for &at in self.polls.iter().chain(std::iter::once(&window)) {
            gaps_ok &= at.saturating_sub(last) <= MAX_POLL_GAP;
            last = at;
        }
        Verdict {
            on_time: gaps_ok && self.polls.len() >= MIN_POLLS,
            figures: self
                .members
                .iter()
                .map(|(label, member)| Figures {
                    label: label.clone(),
                    run_ns: member.run_ns,
                    wait_ns: member.wait_ns,
                    busy_polls: member.busy_polls,
                })
                .collect(),
        }
    }
}

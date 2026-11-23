//! When a measured program counts as quiet: the one definition the waits of
//! `measure` go through (the hidden settle before the first show, each
//! show's and hide's settle, each typed key's and each step's). The settle
//! after start-up's end, before the idle window, is still a fixed wall time
//! (`--settle`): a program asleep on a timer of its own (rinch's idle trim,
//! two seconds after the last frame) is quiet until the timer fires, so a
//! quiet window there would let that work into the idle window, and it is
//! to wait for what the program says instead.
//!
//! # Quiet
//!
//! A window of [`WINDOW`] is quiet when, for every watched process, its
//! threads together ran less than [`QUIET_RUN`] and waited on a run queue
//! less than [`QUIET_WAIT`] (`/proc/<pid>/task/*/schedstat`), and none of
//! its threads was running, runnable or blocked on I/O (state `R` or `D`)
//! at any of the window's polls, made every [`POLL`]; and the program
//! prepared no frame for a hidden window in it. A program that sleeps does
//! neither; one starved of CPU waits, and is runnable.
//!
//! The state is part of it because the kernel adds a thread's run-queue
//! wait only when the thread next gets the CPU: one starved through a
//! whole window shows no wait in it, and is `R` at every poll.
//!
//! # The watched processes
//!
//! - **The program**: its own threads. Its child processes are not counted:
//!   a rinch component starts none that its windows wait on (the launcher
//!   starts applications, which are what a step does, not what it waits
//!   for). A program that waited on a helper process of its own would need
//!   that helper watched too.
//! - **The session's compositor and its descendants**: a program that
//!   waits for its compositor (a configure, a frame callback) sleeps while
//!   a starved compositor is runnable, so its own threads alone would call
//!   that quiet. Whatever the compositor itself starts (an Xwayland, a
//!   helper) is its descendant; the session's clients are started by
//!   xtask, not by the compositor, so they are not.
//! - **The session's bus daemon**, which a program waits on at its start
//!   (its bus name, its readiness) and for any reply.
//!
//! What each nested session held, listed inside it on 2026-11-23 (the
//! compositor's children and theirs, and the bus daemon's name), with
//! dbus-daemon 1.16.2:
//!
//! - KWin 6.6.7: no descendants.
//! - sway 1.11: one descendant, `swaybg`, the background it starts. Idle,
//!   it is quiet: twenty one-second samples at a load of 71, its one
//!   thread asleep at each, 0 ms run and 0 ms waited, which the sway
//!   measurements rest on.
//! - labwc 0.9.3: no descendants.
//!
//! So the descendants rule is exercised by sway's `swaybg` today, and by
//! design for any other: a compositor that starts an Xwayland or a helper
//! has it watched without a change here.
//!
//! Threads that start or end inside a window: a thread first seen at a
//! poll counts all it ran and waited (both are zero at its birth); one that
//! ends keeps what its earlier polls counted, losing only its last moments.
//!
//! # The tool's own polls
//!
//! The polls are the tool's, and the tool can be starved too: a window in
//! which fewer than [`MIN_POLLS`] polls came, or two came more than
//! [`MAX_POLL_GAP`] apart, looked too seldom to be called quiet, and is
//! counted and reported as late.
//!
//! # Limits
//!
//! A spell of running, waiting or blocking that falls wholly between two
//! polls, and is short enough to stay under the bounds, is not seen. Block
//! I/O delay accounting (`kernel.task_delayacct`) would count time blocked
//! on disk between polls; it is a system setting, off by default, and is
//! left as it is. And quiet now does not mean nothing is scheduled: what a
//! program has put off on a timer is not seen until the timer fires.

mod proc;
#[cfg(test)]
mod tests;
mod window;

use std::process::Child;
use std::time::{Duration, Instant};

use deskrinch_test_component::Watch;

use self::window::{Polled, Tally, Verdict};
use super::settle::alive;
use crate::error::Error;
use crate::session::interrupt;

/// The length of a quiet window.
pub(crate) const WINDOW: Duration = Duration::from_secs(1);
/// How often a window polls the watched processes.
pub(crate) const POLL: Duration = Duration::from_millis(100);
/// The most a watched process's threads may run in a quiet window.
pub(crate) const QUIET_RUN: Duration = Duration::from_millis(5);
/// The most a watched process's threads may wait on a run queue in a quiet
/// window: the net for a process held off the CPU between polls. Measured
/// on 2026-11-23 on 24 CPUs: an idle rinch program waits 0 ms a second at
/// any load; a stand-in waking twenty times a second, running under
/// [`QUIET_RUN`], waited 15 to 57 ms a second at loads of 40 to 110 (each
/// wake queues behind the machine's work, so the wait of whatever work the
/// run bound allows grows with the load); a starved process waited 400 to
/// 2200 ms a second. A starvation that lasts the whole window shows no wait
/// at all until the process runs again, and is caught by its state at the
/// polls, whatever this bound.
pub(crate) const QUIET_WAIT: Duration = Duration::from_millis(100);
/// The fewest polls a window may have and still be quiet.
pub(crate) const MIN_POLLS: usize = 8;
/// The longest gap between two polls (or the window's start and its first
/// poll, or its last poll and its end) a quiet window may have.
pub(crate) const MAX_POLL_GAP: Duration = Duration::from_millis(250);

/// The processes besides the program that a quiet window watches: the
/// session's compositor (with its descendants) and its bus daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Neighbours {
    /// The compositor's process id.
    pub(crate) compositor: u32,
    /// The bus daemon's process id.
    pub(crate) bus: u32,
}

/// Everything a quiet window watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Watched {
    /// The measured program's process id.
    pub(crate) program: u32,
    /// The session's processes it may wait on.
    pub(crate) neighbours: Neighbours,
}

impl Watched {
    /// Every watched process's threads, now.
    fn poll(&self) -> Polled {
        let mut polled = vec![
            ("the program".to_owned(), proc::read_tasks(self.program)),
            (
                "the compositor".to_owned(),
                proc::read_tasks(self.neighbours.compositor),
            ),
            (
                "the session's bus".to_owned(),
                proc::read_tasks(self.neighbours.bus),
            ),
        ];
        for pid in proc::Children::detect().descendants(self.neighbours.compositor) {
            polled.push((
                format!("the compositor's child {pid} ({})", command_name(pid)),
                proc::read_tasks(pid),
            ));
        }
        polled
    }
}

/// A process's command name, or `?` once it has gone.
fn command_name(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map_or_else(|_| "?".to_owned(), |name| name.trim_end().to_owned())
}

/// A wait that ended quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Quieted {
    /// How long it took.
    pub(crate) took: Duration,
    /// How many of its windows were thrown out because the tool's own polls
    /// came late.
    pub(crate) late_windows: u32,
}

/// A wait that reached its limit without a quiet window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unquiet {
    /// How long it waited.
    pub(crate) waited: Duration,
    /// Its last window.
    pub(crate) last: Verdict,
    /// Frames the program prepared in its last window.
    pub(crate) prepared: u64,
    /// How many windows were thrown out as late.
    pub(crate) late_windows: u32,
}

impl Unquiet {
    /// What was not quiet in its last window, in words.
    pub(crate) fn said(&self) -> String {
        let mut parts: Vec<String> = self
            .last
            .figures
            .iter()
            .map(|f| {
                format!(
                    "{} ran {} and waited to run {}, busy at {} of its polls",
                    f.label,
                    millis(f.run_ns),
                    millis(f.wait_ns),
                    f.busy_polls,
                )
            })
            .collect();
        parts.push(format!("the program prepared {} frame(s)", self.prepared));
        if !self.last.on_time {
            parts.push("the tool's own polls came late".to_owned());
        }
        format!("in the last window, {}", parts.join("; "))
    }
}

/// `ns` nanoseconds as milliseconds to a tenth: `12.3 ms`.
fn millis(ns: u64) -> String {
    format!("{}.{} ms", ns / 1_000_000, ns / 100_000 % 10)
}

/// Waits, for at most `limit`, for a quiet window of `watched`. Fails if
/// the program exits or xtask is interrupted; a limit reached is
/// [`Unquiet`], for the caller to fail or to go on with.
pub(crate) fn wait(
    watched: &Watched,
    child: &mut Child,
    watch: &Watch,
    program: &str,
    limit: Duration,
) -> Result<Result<Quieted, Unquiet>, Error> {
    let start = Instant::now();
    let mut late_windows = 0;
    loop {
        let begin = Instant::now();
        let prepared_before = watch.prepared();
        let mut tally = Tally::new(&watched.poll());
        let mut next = POLL;
        while begin.elapsed() < WINDOW {
            interrupt::check()?;
            alive(child, watch, program)?;
            std::thread::sleep(next.saturating_sub(begin.elapsed()));
            tally.add(begin.elapsed(), &watched.poll());
            next += POLL;
        }
        let verdict = tally.verdict(begin.elapsed());
        let prepared = watch.prepared() - prepared_before;
        if verdict.quiet() && prepared == 0 {
            return Ok(Ok(Quieted {
                took: start.elapsed(),
                late_windows,
            }));
        }
        late_windows += u32::from(!verdict.on_time);
        if start.elapsed() >= limit {
            return Ok(Err(Unquiet {
                waited: start.elapsed(),
                last: verdict,
                prepared,
                late_windows,
            }));
        }
    }
}

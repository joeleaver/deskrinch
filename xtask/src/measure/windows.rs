//! `--windows N`: measuring a program that opens several windows (one per
//! output, as the wallpaper does), from rinch's per-window `RINCH_PERF`
//! lines ([`deskrinch_test_component::perf`]). Not to be confused with
//! `--window` ([`super::window`]), a window the measurement opens itself.
//!
//! Without `--windows` a run reads the main window's frames, as it always
//! has ([`Expect::Main`]). With it ([`Expect::Windows`]):
//!
//! - Start-up ends when the windows expected have each presented a first
//!   frame. A run where fewer have within the first-frame patience fails,
//!   naming each window seen; one with more open then fails too.
//! - Each window is named from its `opened` line, its title and output
//!   ([`name`]); ids are not used, since they can differ from run to run and
//!   rows are joined across runs by name. Two windows of one name fail the
//!   run, since their rows could not be told apart.
//! - Each window's row at start-up is the process's CPU when its first
//!   frame was seen: cumulative, work for every window included, and the
//!   report says so under those rows.
//! - The idle window's frames are given per window and in all.
//! - The windows must stay the ones start-up ended with: a window that
//!   closes, or one that opens (an output turned off and on again), fails
//!   the run as outside what `measure` measures ([`WindowsError::Changed`]),
//!   since the start-up and idle figures would then mix windows.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use deskrinch_test_component::{Mark, PerfWindow, SinceMark, Watch, WindowState};

use super::report::{Run, Unit};
use crate::error::Error;

/// Which windows' frames a run reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expect {
    /// The main window's, from rinch's unlabelled lines: no `--windows`.
    Main,
    /// Exactly this many windows, whichever they are.
    Windows(usize),
}

impl Expect {
    /// What `--windows` (`None` without it) asks for.
    pub(crate) fn of(windows: Option<u32>) -> Self {
        windows.map_or(Self::Main, |n| {
            Self::Windows(usize::try_from(n).unwrap_or(usize::MAX))
        })
    }

    /// Whether start-up has ended: the main window, or each window
    /// expected, has presented a first frame.
    pub(crate) fn started(self, watch: &Watch) -> bool {
        match self {
            Self::Main => watch.has_first_frame(),
            Self::Windows(n) => watch.wait_first_frames(n, Duration::ZERO).is_some(),
        }
    }

    /// Waits up to `timeout` for start-up to end, or for a window's first
    /// frame before it ends (so that each is seen when it comes).
    pub(crate) fn wait(self, watch: &Watch, timeout: Duration) {
        match self {
            Self::Main => {
                watch.wait_first_frame(timeout);
            }
            Self::Windows(_) => {
                let presented = first_frames(watch).len();
                drop(watch.wait_first_frames(presented + 1, timeout));
            }
        }
    }

    /// The frames presented so far: the main window's, or every window's.
    pub(crate) fn frames(self, watch: &Watch) -> u64 {
        match self {
            Self::Main => watch.frames(),
            Self::Windows(_) => watch
                .since(&Mark::default())
                .iter()
                .map(|since| since.frames)
                .sum(),
        }
    }
}

/// The error for `program`, watched by `watch`, whose windows are wrong.
pub(crate) fn error(program: &str, watch: &Watch, source: WindowsError) -> Error {
    Error::MeasuredWindows {
        program: program.to_owned(),
        source,
        output: watch.tail(),
    }
}

/// The error for `program`, watched by `watch`, whose start-up did not end
/// within `waited`: no first frame, or too few windows' first frames.
pub(crate) fn no_start(expect: Expect, program: &str, watch: &Watch, waited: Duration) -> Error {
    match expect {
        Expect::Main => Error::NoFirstFrame {
            program: program.to_owned(),
            waited,
            output: watch.tail(),
        },
        Expect::Windows(n) => error(program, watch, too_few(&watch.windows(), n, waited)),
    }
}

/// What went wrong with a program's windows.
#[derive(Debug, thiserror::Error)]
pub(crate) enum WindowsError {
    /// Fewer windows than expected presented a first frame in time.
    #[error(
        "{presented} of the {expected} windows expected presented a first frame within {} s; \
         the windows seen:\n{seen}",
        waited.as_secs()
    )]
    TooFew {
        /// How many did.
        presented: usize,
        /// How many were expected.
        expected: usize,
        /// The patience.
        waited: Duration,
        /// Each window seen, one a line.
        seen: String,
    },
    /// More windows were open at the end of start-up than expected.
    #[error("{open} windows open where {expected} were expected:\n{seen}")]
    TooMany {
        /// How many were.
        open: usize,
        /// How many were expected.
        expected: usize,
        /// Each window open, one a line.
        seen: String,
    },
    /// Two windows have one name.
    #[error("two windows are named {name}: their rows could not be told apart")]
    SameName {
        /// The name.
        name: String,
    },
    /// A window closed or opened after start-up.
    #[error(
        "its windows changed after start-up:\n{changes}\nA program whose windows close or open \
         while it is measured (an output turned off and on again) is outside what measure \
         measures: its start-up and idle figures would mix windows."
    )]
    Changed {
        /// Each window that closed or opened, one a line.
        changes: String,
    },
}

/// A window's name in a report: its title, quoted, and the output its
/// `opened` line names; the main window, which has no such line, is `main
/// window`.
pub(crate) fn name(state: &WindowState) -> String {
    match &state.opened {
        None => "main window".to_owned(),
        Some(opened) => match &opened.output {
            Some(output) => format!("{:?} on {output}", opened.title),
            None => format!("{:?}", opened.title),
        },
    }
}

/// The windows that have presented a frame.
fn first_frames(watch: &Watch) -> Vec<WindowState> {
    watch
        .windows()
        .into_iter()
        .filter(|state| state.first_frame.is_some())
        .collect()
}

/// Each window seen, one a line, with whether it has presented.
fn seen(windows: &[WindowState]) -> String {
    windows
        .iter()
        .map(|state| {
            let presented = if state.first_frame.is_some() {
                "presented"
            } else {
                "opened, no frame presented"
            };
            format!("  {}: {presented}", name(state))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The fault in `windows` open when start-up timed out, `expected` wanted.
pub(crate) fn too_few(windows: &[WindowState], expected: usize, waited: Duration) -> WindowsError {
    WindowsError::TooFew {
        presented: windows.iter().filter(|s| s.first_frame.is_some()).count(),
        expected,
        waited,
        seen: seen(windows),
    }
}

/// The CPU time of the process when each window's first frame was seen,
/// gathered while start-up runs.
#[derive(Debug, Default)]
pub(crate) struct Arrivals {
    seen: Vec<(PerfWindow, u64)>,
}

impl Arrivals {
    /// Records, for each window that has presented since the last call,
    /// the process's CPU time now (`cpu`, read once if any has).
    pub(crate) fn record(&mut self, watch: &Watch, cpu: impl FnOnce() -> Option<u64>) {
        let new: Vec<PerfWindow> = first_frames(watch)
            .iter()
            .map(|state| state.window)
            .filter(|window| self.seen.iter().all(|(seen, _)| seen != window))
            .collect();
        if new.is_empty() {
            return;
        }
        let Some(cpu) = cpu() else { return };
        self.seen
            .extend(new.into_iter().map(|window| (window, cpu)));
    }
}

/// The windows start-up ended with, by name, to name and check them by
/// later in the run.
#[derive(Debug)]
pub(crate) struct Started {
    /// Each window open at the end of start-up, and its name.
    open: Vec<(PerfWindow, String)>,
    /// Every window seen by then, closed ones included.
    known: BTreeSet<PerfWindow>,
}

impl Started {
    /// The windows open when start-up ended, checked against `expected`.
    pub(crate) fn check(windows: &[WindowState], expected: usize) -> Result<Self, WindowsError> {
        if windows.len() > expected {
            return Err(WindowsError::TooMany {
                open: windows.len(),
                expected,
                seen: seen(windows),
            });
        }
        let mut open: Vec<(PerfWindow, String)> = windows
            .iter()
            .map(|state| (state.window, name(state)))
            .collect();
        open.sort_by(|a, b| a.1.cmp(&b.1));
        if let Some(pair) = open.windows(2).find(|pair| pair[0].1 == pair[1].1) {
            return Err(WindowsError::SameName {
                name: pair[0].1.clone(),
            });
        }
        let known = open.iter().map(|(window, _)| *window).collect();
        Ok(Self { open, known })
    }

    /// The same, with every window seen by then (closed ones included,
    /// from `watch`).
    pub(crate) fn of(watch: &Watch, expected: usize) -> Result<Self, WindowsError> {
        let mut started = Self::check(&watch.windows(), expected)?;
        started.known.extend(
            watch
                .since(&Mark::default())
                .iter()
                .map(|since| since.window),
        );
        Ok(started)
    }

    /// The start-up rows: each window's arrival, by name, and the remark
    /// under them; the wall-clock ones likewise, from `start`.
    pub(crate) fn push_startup(
        &self,
        run: &mut Run,
        arrivals: &Arrivals,
        windows: &[WindowState],
        start: Instant,
    ) {
        let mut last = None;
        for (window, name) in &self.open {
            if let Some((_, cpu)) = arrivals.seen.iter().find(|(seen, _)| seen == window) {
                let row = format!("process CPU when {name} first presented");
                run.push(row.clone(), Unit::CpuNs, *cpu);
                last = Some(row);
            }
        }
        if let Some(row) = last {
            run.remark(
                row,
                "(cumulative: the process's CPU by then, work for every window included, not \
                 what that window cost)",
            );
        }
        let mut last = None;
        for (window, name) in &self.open {
            let first = windows
                .iter()
                .find(|state| state.window == *window)
                .and_then(|state| state.first_frame);
            if let Some(at) = first {
                let row = format!("wall time when {name} first presented");
                let nanos = u64::try_from(at.saturating_duration_since(start).as_nanos())
                    .unwrap_or(u64::MAX);
                run.push(row.clone(), Unit::WallNs, nanos);
                last = Some(row);
            }
        }
        if let Some(row) = last {
            run.remark(
                row,
                "(from the start: when that window came, not how long it took)",
            );
        }
    }

    /// The idle window's frames, `since` its start ([`Watch::since`]): in
    /// all, then each window's.
    pub(crate) fn push_idle_frames(&self, run: &mut Run, since: &[SinceMark]) -> u64 {
        let frames_of = |window: PerfWindow| {
            since
                .iter()
                .find(|s| s.window == window)
                .map_or(0, |s| s.frames)
        };
        let total = self.open.iter().map(|(window, _)| frames_of(*window)).sum();
        run.push("frames in the idle window", Unit::Count, total);
        for (window, name) in &self.open {
            run.push(format!("  of it {name}"), Unit::Count, frames_of(*window));
        }
        total
    }

    /// Fails if the windows are not those start-up ended with: one closed,
    /// or one opened since.
    pub(crate) fn unchanged(&self, watch: &Watch) -> Result<(), WindowsError> {
        let open = watch.windows();
        let mut changes: Vec<String> = Vec::new();
        for (window, name) in &self.open {
            if open.iter().all(|state| state.window != *window) {
                changes.push(format!("  closed: {name}"));
            }
        }
        let all = watch.since(&Mark::default());
        for since in all.iter().filter(|s| !self.known.contains(&s.window)) {
            let state = open.iter().find(|state| state.window == since.window);
            let name = state.map_or_else(|| "a window, closed again".to_owned(), name);
            changes.push(format!("  opened: {name}"));
        }
        if changes.is_empty() {
            Ok(())
        } else {
            Err(WindowsError::Changed {
                changes: changes.join("\n"),
            })
        }
    }
}

#[cfg(test)]
mod tests;

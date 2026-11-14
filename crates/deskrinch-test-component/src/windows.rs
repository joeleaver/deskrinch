//! Every window a rinch program has made, from its `RINCH_PERF` lines
//! ([`crate::perf`]): what its `opened` line said, its presented and its
//! prepared frames (counted apart), when its first frame was presented, and
//! whether it has closed.
//!
//! The order the lines come in is rinch's: a window's `opened` line is
//! printed when its document is mounted, before its OS window is made and
//! before it can paint, and its `closed` line after its window and
//! document are gone; all of them on standard error, from the main thread.
//! The record does not depend on that order all the same: a frame of a
//! window not yet named starts its record, a later `opened` line names it,
//! and lines after a window's `closed` still count, the window staying
//! closed. The main window has no `opened` line; its `closed` line names an
//! id seen in no other line, which is how it is told.

use std::time::Instant;

use crate::perf::{Opened, PerfLine, PerfWindow};

/// One window, as its lines so far describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowState {
    /// Which window.
    pub window: PerfWindow,
    /// What its `opened` line said; `None` for the main window, which has
    /// none.
    pub opened: Option<Opened>,
    /// Its presented frames.
    pub frames: u64,
    /// Its prepared frames (painted while hidden, not presented).
    pub prepared: u64,
    /// When its first presented frame came.
    pub first_frame: Option<Instant>,
    /// Whether it has closed.
    pub closed: bool,
}

/// Every window's presented and prepared frames at one moment
/// ([`crate::Watch::mark`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mark {
    counts: Vec<(PerfWindow, u64, u64)>,
}

/// One window's frames since a [`Mark`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinceMark {
    /// Which window.
    pub window: PerfWindow,
    /// Its presented frames since the mark.
    pub frames: u64,
    /// Its prepared frames since the mark.
    pub prepared: u64,
}

/// Every window seen, in the order they were first seen.
#[derive(Debug, Default)]
pub(crate) struct Windows {
    all: Vec<WindowState>,
}

impl Windows {
    /// Records `line`, read at `now`.
    pub(crate) fn record(&mut self, line: &PerfLine, now: Instant) {
        match line {
            PerfLine::Frame { window, .. } => {
                let state = self.state(*window);
                state.frames += 1;
                state.first_frame.get_or_insert(now);
            }
            PerfLine::Prepared { window, .. } => self.state(*window).prepared += 1,
            PerfLine::Opened(opened) => {
                self.state(PerfWindow::Id(opened.id)).opened = Some(opened.clone());
            }
            PerfLine::Closed { id } => {
                let known = self
                    .all
                    .iter()
                    .any(|state| state.window == PerfWindow::Id(*id));
                // An id in no other line is the main window's.
                let window = if known {
                    PerfWindow::Id(*id)
                } else {
                    PerfWindow::Main
                };
                self.state(window).closed = true;
            }
        }
    }

    /// The record of `window`, started if it has none.
    fn state(&mut self, window: PerfWindow) -> &mut WindowState {
        if let Some(at) = self.all.iter().position(|state| state.window == window) {
            return &mut self.all[at];
        }
        self.all.push(WindowState {
            window,
            opened: None,
            frames: 0,
            prepared: 0,
            first_frame: None,
            closed: false,
        });
        let last = self.all.len() - 1;
        &mut self.all[last]
    }

    /// `window`'s record, if it has one.
    pub(crate) fn get(&self, window: PerfWindow) -> Option<&WindowState> {
        self.all.iter().find(|state| state.window == window)
    }

    /// The windows open now, in the order they were first seen.
    pub(crate) fn open(&self) -> Vec<WindowState> {
        self.all
            .iter()
            .filter(|state| !state.closed)
            .cloned()
            .collect()
    }

    /// Every window's counts now.
    pub(crate) fn mark(&self) -> Mark {
        Mark {
            counts: self
                .all
                .iter()
                .map(|state| (state.window, state.frames, state.prepared))
                .collect(),
        }
    }

    /// Every window's frames since `mark`, closed ones and ones seen since
    /// included (counted from none).
    pub(crate) fn since(&self, mark: &Mark) -> Vec<SinceMark> {
        self.all
            .iter()
            .map(|state| {
                let (frames, prepared) = mark
                    .counts
                    .iter()
                    .find(|(window, ..)| *window == state.window)
                    .map_or((0, 0), |&(_, frames, prepared)| (frames, prepared));
                SinceMark {
                    window: state.window,
                    frames: state.frames - frames,
                    prepared: state.prepared - prepared,
                }
            })
            .collect()
    }

    /// The windows that have presented a frame, and when their first came,
    /// earliest first.
    pub(crate) fn first_frames(&self) -> Vec<(PerfWindow, Instant)> {
        let mut first: Vec<_> = self
            .all
            .iter()
            .filter_map(|state| state.first_frame.map(|at| (state.window, at)))
            .collect();
        first.sort_by_key(|&(_, at)| at);
        first
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::perf::parse;

    const FIXTURE: &str = include_str!("../tests/fixtures/two-windows-and-a-prepared-one.txt");

    /// The fixture's lines recorded one millisecond apart from `start`.
    fn recorded(start: Instant) -> Windows {
        let mut windows = Windows::default();
        let lines = FIXTURE.lines().filter_map(parse);
        for (i, line) in (0_u64..).zip(lines) {
            windows.record(&line, start + Duration::from_millis(i));
        }
        windows
    }

    #[test]
    fn a_real_run_leaves_every_window_counted_and_closed_as_it_was() {
        let start = Instant::now();
        let windows = recorded(start);
        let main = windows.get(PerfWindow::Main).unwrap();
        assert_eq!((main.frames, main.prepared, main.closed), (3, 1, false));
        assert_eq!(main.opened, None);
        for id in [2, 3] {
            let state = windows.get(PerfWindow::Id(id)).unwrap();
            assert_eq!((state.frames, state.prepared, state.closed), (2, 0, true));
            assert_eq!(
                state.opened.as_ref().map(|o| o.app_id.as_deref()),
                Some(Some("org.example.fixture"))
            );
        }
        let hidden = windows.get(PerfWindow::Id(4)).unwrap();
        assert_eq!((hidden.frames, hidden.prepared), (0, 1));
        assert_eq!(
            hidden.first_frame, None,
            "a prepared frame is not presented"
        );
        // At the end only the main window is open.
        let open: Vec<_> = windows.open().iter().map(|s| s.window).collect();
        assert_eq!(open, [PerfWindow::Main]);
        // Window 3 presented first (line 5), then 2, then the main window.
        let first: Vec<_> = windows.first_frames().iter().map(|&(w, _)| w).collect();
        assert_eq!(
            first,
            [PerfWindow::Id(3), PerfWindow::Id(2), PerfWindow::Main]
        );
    }

    #[test]
    fn frames_and_prepared_frames_since_a_mark() {
        let mut windows = Windows::default();
        let now = Instant::now();
        let lines = FIXTURE.lines().filter_map(parse).collect::<Vec<_>>();
        // Up to and including the hidden window's preparation.
        for line in &lines[..5] {
            windows.record(line, now);
        }
        let mark = windows.mark();
        for line in &lines[5..] {
            windows.record(line, now);
        }
        let since = windows.since(&mark);
        let of = |window| since.iter().find(|s| s.window == window).copied().unwrap();
        assert_eq!(of(PerfWindow::Main).frames, 3);
        assert_eq!(of(PerfWindow::Main).prepared, 0);
        assert_eq!(of(PerfWindow::Id(2)).frames, 2);
        assert_eq!(of(PerfWindow::Id(4)).prepared, 0, "nothing prepared since");
        assert_eq!(
            windows
                .since(&windows.mark())
                .iter()
                .map(|s| s.frames)
                .sum::<u64>(),
            0
        );
    }

    #[test]
    fn lines_out_of_rinchs_order_are_still_counted() {
        let mut windows = Windows::default();
        let now = Instant::now();
        // A frame before the window's `opened` line, and one after its
        // `closed` line.
        for line in [
            "[PERF] window 7 frame 1: a",
            r#"[PERF] window 7 opened: title="late""#,
            "[PERF] window 7 closed",
            "[PERF] window 7 frame 2: b",
        ] {
            windows.record(&parse(line).unwrap(), now);
        }
        let state = windows.get(PerfWindow::Id(7)).unwrap();
        assert_eq!(state.frames, 2);
        assert_eq!(
            state.opened.as_ref().map(|o| o.title.as_str()),
            Some("late")
        );
        assert!(state.closed);
        // The main window's `closed` line names an id seen nowhere else.
        windows.record(&parse("[PERF] frame 1: a").unwrap(), now);
        windows.record(&parse("[PERF] window 1 closed").unwrap(), now);
        assert!(windows.get(PerfWindow::Main).unwrap().closed);
        assert!(windows.get(PerfWindow::Id(1)).is_none());
    }
}

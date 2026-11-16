//! What a step of a test cost the panel, read from rinch's own counters on
//! its frame lines: the frames presented since a line of its log, told
//! apart by whether they did reactive work (`signal_notifies`), and the
//! check that a frame repainted and laid nothing out.

use deskrinch_test_component::log::strip_ansi;
use deskrinch_test_component::perf::parse;
use deskrinch_test_component::{PerfLine, PerfWindow, counter};

use crate::support::Panel;

/// The panel's log line to count a step's frames from.
pub(crate) fn step(panel: &Panel) -> usize {
    panel.watch().lines().len()
}

/// The panel's presented frame lines since its log line `from`, as
/// (reactive, not reactive): a frame that notified a signal did reactive
/// work.
pub(crate) fn since(panel: &Panel, from: usize) -> (Vec<String>, Vec<String>) {
    let lines = panel.watch().lines();
    lines[from..]
        .iter()
        .filter(|line| {
            matches!(
                parse(&strip_ansi(line)),
                Some(PerfLine::Frame {
                    window: PerfWindow::Main,
                    ..
                })
            )
        })
        .cloned()
        .partition(|line| counter(line, "signal_notifies").is_some())
}

/// Asserts that rinch's counters on `frame` say it repainted
/// (`paint_frames`) and resolved no layout: rinch's layout pass may be
/// entered, but each entry skipped Taffy because nothing but paint changed
/// (`layout_skipped_paint_only` equal to `layout_resolves`), and no node's
/// layout style changed (`taffy_style_changes` absent: rinch leaves a
/// counter that is 0 out of the line).
pub(crate) fn assert_paint_only(frame: &str, what: &str) {
    assert_eq!(
        counter(frame, "paint_frames"),
        Some(1),
        "{what}: the frame did not repaint: {frame}"
    );
    assert_eq!(
        counter(frame, "layout_skipped_paint_only"),
        counter(frame, "layout_resolves"),
        "{what}: the frame resolved layout: {frame}"
    );
    assert_eq!(
        counter(frame, "taffy_style_changes"),
        None,
        "{what}: the frame changed a layout style: {frame}"
    );
}

/// Asserts that the step since the panel's log line `from` made exactly one
/// frame without reactive work, and that it was paint-only
/// ([`assert_paint_only`]): what a `:hover` change costs. Returns the step's
/// reactive frames, left out of the count.
pub(crate) fn one_paint_only_frame(panel: &Panel, from: usize, what: &str) -> Vec<String> {
    let (reactive, frames) = since(panel, from);
    let [frame] = frames.as_slice() else {
        panic!(
            "{what}: {} frames without reactive work, not one: {frames:#?}; \
             the reactive frames left out: {reactive:#?}",
            frames.len()
        );
    };
    assert_paint_only(frame, what);
    reactive
}

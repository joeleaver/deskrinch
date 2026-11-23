//! A span judged on a bar's repaints, and when the clock voids it.
//!
//! The clock redraws every bar each time its text changes, which the
//! judgment that a bar repainted nothing cannot tell from a repaint for what
//! the span judges. The panel says when it applies a text
//! ([`deskrinch_panel::clock_shows`], before the frame that draws it), so a
//! span is void on the panel's own word, not by the time.

use deskrinch_panel::CLOCK_SHOWS;
use deskrinch_test_component::perf::parse;
use deskrinch_test_component::{PerfLine, PerfWindow};

/// Whether `line` is a frame of the bar `window`.
fn frame_of(line: &str, window: PerfWindow) -> bool {
    matches!(parse(line), Some(PerfLine::Frame { window: drew, .. }) if drew == window)
}

/// Whether `line` is the clock's, saying it applied a text.
fn clock(line: &str) -> bool {
    line.contains(CLOCK_SHOWS)
}

/// Whether, by the panel's log `lines`, the clock applied a text the bar
/// `window` has not drawn yet: its last text applied has no frame of that
/// bar after it. At a panel's start the clock's first text can come after a
/// bar's first frame; a span begins once this is false.
pub(crate) fn pending(lines: &[String], window: PerfWindow) -> bool {
    lines
        .iter()
        .rposition(|line| clock(line))
        .is_some_and(|at| !lines[at..].iter().any(|line| frame_of(line, window)))
}

/// Whether the span of the panel's log `lines` from `from` (its window's
/// first line) to `to` (its bound's first `placed` line) is void for a
/// judgment of the repaints of the bar `window`: the clock applied a text
/// inside it, or applied one before it that the bar had not drawn yet when
/// it began, so that the clock's frame can fall inside it. A text applied
/// after `to` is drawn after it, and not read.
pub(crate) fn void(lines: &[String], window: PerfWindow, from: usize, to: usize) -> bool {
    pending(&lines[..from], window) || lines[from..to].iter().any(|line| clock(line))
}

/// The panel's log lines of a capture.
#[cfg(test)]
fn lines(capture: &str) -> Vec<String> {
    capture.lines().map(str::to_owned).collect()
}

/// Captured from a real run of `outputs::an_output_rescaled_keeps_its_bar`
/// under sway (two outputs at scale 1, the software renderer), the clock at
/// seconds: the panel's lines when a span's bound had said where the bars
/// are, and until the clock's next text after it. The span ran from line 10
/// to line 18, the bound's first `placed` line, judging the bar of window 1;
/// the clock applied a text at line 15, inside it, and the bar drew it at
/// line 17.
#[cfg(test)]
const INSIDE: &str = include_str!("spans/inside.log");

/// Captured from the same run, another pass: the span ran from line 10 to
/// line 15, the bound's first `placed` line, judging the bar of window 1.
/// The clock applied a text at line 3, which that bar drew at line 9, before
/// the span; and one at line 21, after the bound (the bound's edit restarted
/// the clock at minutes).
#[cfg(test)]
const AFTER_THE_BOUND: &str = include_str!("spans/after_the_bound.log");

#[test]
fn a_span_the_clock_applied_a_text_inside_is_void() {
    assert!(void(&lines(INSIDE), PerfWindow::Id(1), 10, 18));
}

#[test]
fn a_span_begun_before_the_bar_drew_the_clocks_last_text_is_void() {
    // From line 9: the text applied at line 3 is drawn at line 9, inside.
    assert!(void(&lines(AFTER_THE_BOUND), PerfWindow::Id(1), 9, 15));
}

#[test]
fn a_text_applied_after_the_bound_does_not_void_the_span() {
    // The span as it ran: the text at line 3 was drawn before it, the one
    // at line 21 comes after its bound.
    assert!(!void(&lines(AFTER_THE_BOUND), PerfWindow::Id(1), 10, 15));
}

#[test]
fn a_span_with_no_text_applied_or_pending_is_not_void() {
    // Lines 16 to 21: no text applied inside, the last before drawn.
    assert!(!void(&lines(AFTER_THE_BOUND), PerfWindow::Id(1), 16, 21));
}

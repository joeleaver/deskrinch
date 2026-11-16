//! What a step of a test cost the panel, read from rinch's own counters on
//! its frame lines: the frames presented since a line of its log, told
//! apart by whether they did reactive work (`signal_notifies`), and the
//! check that a frame repainted and laid nothing out.

use deskrinch_test_component::log::strip_ansi;
use deskrinch_test_component::perf::parse;
use deskrinch_test_component::{PerfLine, PerfWindow, counter};
use deskrinch_test_wait::Deadline;

use crate::support::Panel;

/// The panel's log line to count a step's frames from.
pub(crate) fn step(panel: &Panel) -> usize {
    panel.watch().lines().len()
}

/// The frame lines among `lines` (the panel's log), as (reactive, not
/// reactive): a frame that notified a signal did reactive work, and is
/// kept whatever it painted, since that work is the step's to account for.
///
/// Of the frames without reactive work, a redraw that rinch served from the
/// cached pixels, with nothing dirty (`paint_cached_frames` and no
/// `paint_frames`), drew nothing, and is left out. rinch logs one as a
/// frame line (seen on rinch b31389bb when the pointer left the panel and
/// no `:hover` rule had anything to change).
fn split(lines: &[String]) -> (Vec<String>, Vec<String>) {
    let (reactive, frames): (Vec<String>, Vec<String>) = lines
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
        .partition(|line| counter(line, "signal_notifies").is_some());
    let drawn = frames
        .into_iter()
        .filter(|line| {
            counter(line, "paint_frames").is_some()
                || counter(line, "paint_cached_frames").is_none()
        })
        .collect();
    (reactive, drawn)
}

/// The panel's frame lines since its log line `from`, as [`split`] gives
/// them: what a step that is no hover (a switch of the current workspace)
/// drew.
pub(crate) fn since(panel: &Panel, from: usize) -> (Vec<String>, Vec<String>) {
    split(&panel.watch().lines()[from..])
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

/// The hover's step since the panel's log line `from`: its reactive frames
/// (none when the step was clean, the hover alone), and the line its read
/// stopped at, where the next step's window begins.
///
/// A step with any frame without reactive work must have exactly one, and
/// it must be paint-only ([`assert_paint_only`]): what a `:hover` change
/// costs; anything else fails at once. A step with reactive frames is
/// crowded: drawn for something else the panel follows (the clock's
/// minute, a title, a late arrival), or the hover's restyle batched into
/// one frame with something reactive, when no frame without reactive work
/// is left. A step with no frame that restyled at all fails at once: the
/// hover drew nothing.
fn hover_step(panel: &Panel, from: usize, what: &str) -> (Vec<String>, usize) {
    let lines = panel.watch().lines();
    let (reactive, frames) = split(&lines[from..]);
    match frames.as_slice() {
        [frame] => assert_paint_only(frame, what),
        [] if reactive
            .iter()
            .any(|line| counter(line, "style_resolves").is_some()) => {}
        [] => panic!("{what}: no frame restyled; the reactive frames: {reactive:#?}"),
        _ => panic!(
            "{what}: {} frames without reactive work, not one: {frames:#?}; \
             the reactive frames: {reactive:#?}",
            frames.len()
        ),
    }
    (reactive, lines.len())
}

/// Repeats rounds of the hover's two steps, `onto` (the pointer onto the
/// button) and `off` (off the panel), each given `state` and returning once
/// the screen shows what it did, until a round after the first is clean in
/// both its steps, or the patience runs out. `panel` gives `state`'s panel,
/// whose frame lines are read after each step ([`hover_step`]).
///
/// The steps' windows are contiguous: the first begins where the panel's
/// log stood before the first round, and each later one where the previous
/// step's read stopped, so no frame between two steps goes unread.
///
/// Reactive work the hover causes (a handler that sets a signal, on
/// entering or on leaving, directly or deferred by less than a round)
/// crowds every round: work caused on entering lands in the round's own
/// step onto the button or step off it; work caused on leaving lands in
/// that step off, or in the next round's step onto. The first round is not
/// counted as clean, since no leave comes before its step onto: work
/// deferred from its leave past its own read would be seen only in the
/// round after. The clock's minute, a title or a late arrival crowds a round
/// or two. So the patience running out without a clean round fails, with
/// every round's steps listed.
///
/// The rule's limit: work deferred by longer than a round (the two steps,
/// each a pointer move, the screen showing it, and the screen keeping still
/// for the stability window: 1.7 to 1.9 s a round, measured under sway on
/// rinch f5dd9e4e3 at loads of 9 to 19) can land in
/// a round after the one it belongs to, behind a clean round, and is not
/// seen.
///
/// # Panics
///
/// As above, and as the steps themselves do.
pub(crate) fn until_clean_round<S>(
    state: &mut S,
    panel: impl Fn(&S) -> &Panel,
    mut onto: impl FnMut(&mut S),
    mut off: impl FnMut(&mut S),
) {
    let deadline = Deadline::patience();
    let mut from = step(panel(state));
    let mut rounds = Vec::new();
    for round in 1_u32.. {
        onto(state);
        let (onto_frames, after_onto) = hover_step(panel(state), from, "onto the button");
        off(state);
        let (off_frames, after_off) = hover_step(panel(state), after_onto, "off the panel");
        from = after_off;
        if round > 1 && onto_frames.is_empty() && off_frames.is_empty() {
            return;
        }
        rounds.push((round, ("onto", onto_frames), ("off", off_frames)));
        assert!(
            !deadline.passed(),
            "no clean round after the first: each had reactive work, which a hover would \
             cause if it set a signal; the rounds: {rounds:#?}"
        );
    }
}

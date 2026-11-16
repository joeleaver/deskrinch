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

/// What a hover's step (the pointer onto a button, or off the panel) was,
/// by its frames.
#[derive(Debug)]
pub(crate) enum Step {
    /// Exactly one frame without reactive work, paint-only, and no reactive
    /// frame: the hover alone.
    Clean,
    /// The step had reactive frames: drawn for something else the panel
    /// follows (the clock's minute, a title, a late arrival), or the hover's
    /// restyle batched into one with them. They are kept for a failure's
    /// message.
    Crowded(Vec<String>),
}

/// The hover's step since the panel's log line `from`.
///
/// A step with any frame without reactive work must have exactly one, and
/// it must be paint-only ([`assert_paint_only`]): what a `:hover` change
/// costs; anything else fails at once. A step whose only frames are
/// reactive, one of them restyling, is crowded: rinch batched the hover's
/// restyle into a frame with something reactive. A step with no frame that
/// restyled fails at once: the hover drew nothing.
pub(crate) fn hover_step(panel: &Panel, from: usize, what: &str) -> Step {
    let (reactive, frames) = since(panel, from);
    match frames.as_slice() {
        [frame] => {
            assert_paint_only(frame, what);
            if reactive.is_empty() {
                Step::Clean
            } else {
                Step::Crowded(reactive)
            }
        }
        [] if reactive
            .iter()
            .any(|line| counter(line, "style_resolves").is_some()) =>
        {
            Step::Crowded(reactive)
        }
        [] => panic!("{what}: no frame restyled; the reactive frames: {reactive:#?}"),
        _ => panic!(
            "{what}: {} frames without reactive work, not one: {frames:#?}; \
             the reactive frames: {reactive:#?}",
            frames.len()
        ),
    }
}

/// Repeats the hover's two steps, `onto` and `off`, each given `state` and
/// giving its [`Step`], until a clean step onto and a clean step off have
/// both been seen, or the patience runs out.
///
/// A step that is crowded every time is the hover's own reactive work
/// (a handler that sets a signal, directly or deferred, on entering or on
/// leaving); the clock or a late arrival crowds a step or two. So the
/// patience running out without a clean step in one direction fails,
/// naming that direction, with every step listed.
///
/// # Panics
///
/// As above, and as the steps themselves do.
pub(crate) fn until_both_clean<S>(
    state: &mut S,
    mut onto: impl FnMut(&mut S) -> Step,
    mut off: impl FnMut(&mut S) -> Step,
) {
    let deadline = Deadline::patience();
    let (mut clean_onto, mut clean_off) = (false, false);
    let mut steps = Vec::new();
    loop {
        match onto(state) {
            Step::Clean => clean_onto = true,
            Step::Crowded(frames) => steps.push(("onto", frames)),
        }
        match off(state) {
            Step::Clean => clean_off = true,
            Step::Crowded(frames) => steps.push(("off", frames)),
        }
        if clean_onto && clean_off {
            return;
        }
        let never: Vec<&str> = [("onto", clean_onto), ("off", clean_off)]
            .into_iter()
            .filter_map(|(direction, clean)| (!clean).then_some(direction))
            .collect();
        assert!(
            !deadline.passed(),
            "no clean step {never:?} the button: every one had reactive work, which a hover \
             would cause if it set a signal; the crowded steps: {steps:#?}"
        );
    }
}

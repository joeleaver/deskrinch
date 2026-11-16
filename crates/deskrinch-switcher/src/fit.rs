//! [`fit`]: which buttons of each run are drawn in the room the panel gives
//! the switcher.

use crate::{Button, Run};

/// The room, and the sizes of what is drawn in it, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Room {
    /// The switcher's width.
    pub width: f32,
    /// A button's width.
    pub button: f32,
    /// Between two buttons, and between a button and a count.
    pub gap: f32,
    /// Between two runs.
    pub run_gap: f32,
    /// A count of the workspaces left out at one end of a run.
    pub count_width: f32,
}

/// One run as drawn: the buttons that fit, and how many are left out at
/// either end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fitted<Id> {
    /// Left out before the buttons shown.
    pub before: usize,
    /// The buttons shown, in order.
    pub shown: Vec<Button<Id>>,
    /// Left out after them.
    pub after: usize,
}

/// Which buttons of `runs` fit in `room`. The runs share the room equally,
/// less the gaps between them. A run's buttons all fit, or else as many as
/// fit beside the counts of those left out, as near the current one as can
/// be (the furthest from it left out), and never fewer than the current
/// one (or the first, where none is current) alone.
#[must_use]
pub fn fit<Id: Clone>(runs: &[Run<Id>], room: Room) -> Vec<Fitted<Id>> {
    let share = (room.width - gaps(runs.len(), room.run_gap)) / count_f32(runs.len().max(1));
    runs.iter().map(|run| fit_run(run, share, room)).collect()
}

/// `run`'s buttons in `width`.
fn fit_run<Id: Clone>(run: &Run<Id>, width: f32, room: Room) -> Fitted<Id> {
    let count = run.buttons.len();
    let centre = run.buttons.iter().position(|b| b.current).unwrap_or(0);
    let window = (1..=count)
        .rev()
        .map(|kept| around(centre, kept, count))
        .find(|&(start, kept)| needed(start, kept, count, room) <= width)
        .unwrap_or_else(|| around(centre, count.min(1), count));
    let (start, kept) = window;
    Fitted {
        before: start,
        shown: run.buttons[start..start + kept].to_vec(),
        after: count - start - kept,
    }
}

/// The `kept` buttons of `count` centred on the one at `centre`, as near
/// the middle as the ends allow: their first index and their number.
fn around(centre: usize, kept: usize, count: usize) -> (usize, usize) {
    let start = centre.saturating_sub(kept.saturating_sub(1) / 2);
    (start.min(count - kept), kept)
}

/// The width of `kept` buttons from `start` among `count`, with the counts
/// of those left out at either end.
fn needed(start: usize, kept: usize, count: usize, room: Room) -> f32 {
    let counts = [start > 0, start + kept < count]
        .into_iter()
        .filter(|&left_out| left_out)
        .count();
    count_f32(kept) * room.button
        + gaps(kept, room.gap)
        + count_f32(counts) * (room.gap + room.count_width)
}

/// The gaps between `count` things `gap` apart.
fn gaps(count: usize, gap: f32) -> f32 {
    count_f32(count.saturating_sub(1)) * gap
}

/// `count` as a width multiplier. A switcher has tens of workspaces at
/// most (the workspaces crate lists 256), far below where `f32` loses
/// whole numbers.
fn count_f32(count: usize) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "at most 256 workspaces")]
    let count = count as f32;
    count
}

#[cfg(test)]
#[path = "fit_tests.rs"]
mod tests;

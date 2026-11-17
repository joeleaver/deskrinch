//! The column read from the screen.
//!
//! The column is found by what changed: every output is captured, still,
//! before the daemon shows anything, and the column is the pixels that
//! differ since. The popups are the runs of rows that differ inside it,
//! apart from each other by the column's gaps, which paint nothing; the
//! count line is the run under them that spans only a few words.

use std::time::Duration;

use deskrinch_test_client::Compositor;
use deskrinch_test_screen::{Image, Rgba, Screen};
use deskrinch_test_wait::{Deadline, wait_for};

use super::{Notifications, PADDING, WIDTH};

/// How long the screen must not change to count as still under
/// `compositor`. Under KWin, longer than the longest close animation it runs
/// by default (its fading popups, 600 ms). sway and labwc animate nothing:
/// there a few frames' time is enough (six at 60 Hz). A capture taken
/// before a change has landed is no harm: each wait on the screen holds the
/// column to what it waits for, and looks again.
fn still_for(compositor: Compositor) -> Duration {
    match compositor {
        Compositor::Kwin => Duration::from_millis(600),
        Compositor::Sway | Compositor::Labwc => Duration::from_millis(100),
    }
}

/// How far a colour on the screen may be from the one looked for, per
/// channel: a compositor's rounding, far less than between two colours.
const TOLERANCE: u8 = 8;

/// Each output's picture, by name.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Pictures(Vec<(String, Image)>);

impl Pictures {
    /// Every output's picture, or with `only` that output's alone.
    fn take(screen: &mut Screen, only: Option<&str>) -> Self {
        Self(
            screen
                .outputs()
                .into_iter()
                .filter(|name| only.is_none_or(|only| only == name))
                .map(|name| {
                    let image = screen.capture(&name).unwrap();
                    (name, image)
                })
                .collect(),
        )
    }

    /// Each output (or with `only` that output alone) once it is still: the
    /// same in two captures apart by the compositor's [`still_for`].
    pub(super) fn still(screen: &mut Screen, only: Option<&str>) -> Self {
        let deadline = Deadline::patience();
        let gap = still_for(Compositor::current());
        let mut last = Self::take(screen, only);
        loop {
            std::thread::sleep(gap);
            let now = Self::take(screen, only);
            if now == last {
                return now;
            }
            assert!(!deadline.passed(), "the screen never stood still");
            last = now;
        }
    }
}

/// The column on the screen: the output it is on and the box of its popups
/// there, in the screen's pixels, inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Column {
    pub(crate) output: String,
    pub(crate) left: u32,
    pub(crate) top: u32,
    pub(crate) right: u32,
    pub(crate) bottom: u32,
    /// Each popup's first and last rows, top to bottom.
    pub(crate) popups: Vec<(u32, u32)>,
    /// The box of the count line's text, while it shows: left, top, right
    /// and bottom, inclusive.
    pub(crate) count_line: Option<(u32, u32, u32, u32)>,
    /// The output's size in its pixels.
    pub(crate) size: (u32, u32),
    /// What changed, coarsely, for a failure to show: every eighth row
    /// that changed, with the spans of its columns that did.
    pub(crate) map: Vec<String>,
}

/// The spans of `row`'s pixels that differ between `before` and `now`, as
/// `"y: x0-x1 x2-x3"`; `None` when none does.
fn spans(before: &Image, now: &Image, row: u32) -> Option<String> {
    let mut spans: Vec<(u32, u32)> = Vec::new();
    for x in 0..now.width() {
        if before.pixel(x, row) != now.pixel(x, row) {
            match spans.last_mut() {
                Some((_, end)) if *end + 1 == x => *end = x,
                _ => spans.push((x, x)),
            }
        }
    }
    (!spans.is_empty()).then(|| {
        let spans: Vec<String> = spans.iter().map(|(a, b)| format!("{a}-{b}")).collect();
        format!("{row}: {}", spans.join(" "))
    })
}

/// A run of rows that changed: its first and last rows, and the leftmost
/// and rightmost columns that changed in it.
#[derive(Debug, Clone, Copy)]
struct Run {
    first: u32,
    last: u32,
    left: u32,
    right: u32,
}

/// The column on the output `output`, which showed `before` and shows `now`:
/// the runs of rows whose pixels differ, the popups among them (those that
/// span most of a popup's width at `scale`; the count line is a few words)
/// and the popups' box (the count line's, when no popup shows); `None` when
/// nothing differs.
fn changed(output: &str, before: &Image, now: &Image, scale: u32) -> Option<Column> {
    let mut runs: Vec<Run> = Vec::new();
    for y in 0..now.height() {
        let differs: Vec<u32> = (0..now.width())
            .filter(|x| before.pixel(*x, y) != now.pixel(*x, y))
            .collect();
        let (Some(&left), Some(&right)) = (differs.first(), differs.last()) else {
            continue;
        };
        match runs.last_mut() {
            Some(run) if run.last + 1 == y => {
                run.last = y;
                run.left = run.left.min(left);
                run.right = run.right.max(right);
            }
            _ => runs.push(Run {
                first: y,
                last: y,
                left,
                right,
            }),
        }
    }
    // The column lies in the output's rightmost `WIDTH`: what changed
    // further left (a panel along the top, which spans the output) is not
    // the column's.
    let column_left = now.width().saturating_sub(WIDTH * scale);
    let popup = (WIDTH - 2 * PADDING) * scale * 3 / 4;
    let (popups, others): (Vec<Run>, Vec<Run>) = runs
        .into_iter()
        .filter(|run| run.left >= column_left)
        .partition(|run| run.right - run.left >= popup);
    let boxed = if popups.is_empty() { &others } else { &popups };
    let left = boxed.iter().map(|run| run.left).min()?;
    let right = boxed.iter().map(|run| run.right).max()?;
    Some(Column {
        output: output.to_owned(),
        left,
        top: boxed.first()?.first,
        right,
        bottom: boxed.last()?.last,
        popups: popups.iter().map(|run| (run.first, run.last)).collect(),
        count_line: others
            .last()
            .map(|run| (run.left, run.first, run.right, run.last)),
        size: (now.width(), now.height()),
        map: (0..now.height())
            .step_by(8)
            .filter_map(|row| spans(before, now, row))
            .collect(),
    })
}

impl Notifications {
    /// The columns on the screen now, once it is still: one per output
    /// where something changed since before the daemon showed anything.
    ///
    /// Until a column has been found every output is captured, so that the
    /// first wait sees the column on one output and nowhere else; from then
    /// on, only the output it was found on (a capture of every output costs
    /// a few hundred milliseconds a time, and the later waits are about the
    /// column's content).
    fn columns(&mut self) -> Vec<Column> {
        let on = self.on.clone();
        let now = Pictures::still(&mut self.screen, on.as_deref());
        self.before
            .0
            .iter()
            .filter(|(name, _)| on.as_ref().is_none_or(|on| on == name))
            .zip(&now.0)
            .filter_map(|((name, was), (_, is))| changed(name, was, is, self.scale))
            .collect()
    }

    /// Waits until the column on the screen is one `accept` takes, and
    /// returns it.
    pub(crate) fn column(&mut self, what: &str, accept: impl Fn(&Column) -> bool) -> Column {
        let column = wait_for(what, || {
            let columns = self.columns();
            match columns.as_slice() {
                [column] if accept(column) => Ok(column.clone()),
                _ => Err((columns, self.said())),
            }
        });
        self.on = Some(column.output.clone());
        column
    }

    /// Waits until the column shows `shown` popups, its count line is there
    /// exactly when some wait, and the daemon's last fit line says `shown`
    /// shown and `waiting` waiting; the column then.
    pub(crate) fn settled(&mut self, shown: usize, waiting: usize) -> Column {
        let what = format!("{shown} shown and {waiting} waiting");
        let (column, _) = self.settled_on(&what, |fit| fit == (shown, waiting));
        column
    }

    /// Waits until the screen, still, agrees with the daemon's last fit
    /// line (as [`settled`](Self::settled) has it) and `accept` takes that
    /// fit; the column and the fit then. A fit the daemon passes through on
    /// its way, said before the screen settles, is never taken.
    pub(crate) fn settled_on(
        &mut self,
        what: &str,
        accept: impl Fn((usize, usize)) -> bool,
    ) -> (Column, (usize, usize)) {
        let settled = wait_for(what, || {
            let columns = self.columns();
            let said = self.fit();
            match (columns.as_slice(), said) {
                ([column], Some((shown, waiting)))
                    if column.popups.len() == shown
                        && column.count_line.is_some() == (waiting > 0)
                        && accept((shown, waiting)) =>
                {
                    Ok((column.clone(), (shown, waiting)))
                }
                _ => Err((columns, said, self.said())),
            }
        });
        self.on = Some(settled.0.output.clone());
        settled
    }

    /// Waits until the screen, every output of it, is as it was before the
    /// daemon showed anything.
    pub(crate) fn wait_gone(&mut self) {
        let before = &self.before;
        let screen = &mut self.screen;
        wait_for("the screen as it was", || {
            let now = Pictures::still(screen, None);
            if now == *before {
                Ok(())
            } else {
                Err("it differs")
            }
        });
    }

    /// The image of `column`'s output now.
    pub(crate) fn picture(&mut self, column: &Column) -> Image {
        self.screen.capture(&column.output).unwrap()
    }

    /// Whether `column` sits in its output's top-right corner: its right and
    /// top edges within the padding (and a shadow's few pixels) of the
    /// output's, and no wider than the column.
    pub(crate) fn top_right(&self, column: &Column) -> bool {
        let (width, _) = column.size;
        let reach = (PADDING + 4) * self.scale;
        column.right + 1 + reach >= width
            && column.top <= reach
            && column.left + WIDTH * self.scale >= width
    }
}

/// Whether the popup in rows `popup` of `image` is drawn whole: the middle
/// of its last row is its border, as the middle of its first row is, and
/// unlike the popup's own colour just inside it.
pub(crate) fn whole(image: &Image, column: &Column, popup: (u32, u32), scale: u32) -> bool {
    let (first, last) = popup;
    let middle = column.left.midpoint(column.right);
    let top = image.pixel(middle, first);
    let bottom = image.pixel(middle, last);
    let inside = image.pixel(middle, first + 4 * scale);
    top.is_some() && top == bottom && top != inside
}

/// Whether `colour` is within [`TOLERANCE`] of `wanted`.
fn near(colour: Rgba, wanted: Rgba) -> bool {
    colour.r.abs_diff(wanted.r) <= TOLERANCE
        && colour.g.abs_diff(wanted.g) <= TOLERANCE
        && colour.b.abs_diff(wanted.b) <= TOLERANCE
}

/// Whether `colour` is clearly red: the theme's red border, whatever its
/// shade.
pub(crate) fn reddish(colour: Rgba) -> bool {
    colour.r >= 180 && colour.g <= 120 && colour.b <= 120
}

/// The box of the pixels near `colour` in rows `rows` of `image`, if any.
pub(crate) fn find(image: &Image, rows: (u32, u32), colour: Rgba) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in rows.0..=rows.1 {
        for x in 0..image.width() {
            if image.pixel(x, y).is_some_and(|p| near(p, colour)) {
                bounds = Some(match bounds {
                    None => (x, y, x, y),
                    Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
                });
            }
        }
    }
    bounds
}

/// Where a popup's close button is: the rightmost mark in its header, a
/// band at its top, other than its border; `(x, y)` in the screen's pixels.
pub(crate) fn close_button(
    image: &Image,
    column: &Column,
    popup: (u32, u32),
    scale: u32,
) -> (u32, u32) {
    let (top, _) = popup;
    // The popup's own colour, inside its border, at the header's left.
    let inner = column.left + 4 * scale;
    let ground = image.pixel(inner, top + 4 * scale).unwrap();
    let band = (top + 6 * scale, top + 40 * scale);
    let edge = column.right.saturating_sub(4 * scale);
    let mut rightmost: Option<(u32, u32)> = None;
    for y in band.0..=band.1 {
        for x in (inner..edge).rev() {
            let p = image.pixel(x, y).unwrap();
            let differs = p.r.abs_diff(ground.r) > 40
                || p.g.abs_diff(ground.g) > 40
                || p.b.abs_diff(ground.b) > 40;
            if differs {
                if rightmost.is_none_or(|(rx, _)| x > rx) {
                    rightmost = Some((x, y));
                }
                break;
            }
        }
    }
    let (x, y) = rightmost.expect("a mark in the popup's header");
    // The mark's middle: a few of its pixels left of its right edge.
    (x.saturating_sub(2 * scale), y + 2 * scale)
}

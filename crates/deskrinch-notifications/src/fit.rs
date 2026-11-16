//! Which popups the column shows when not all fit, and which wait.
//!
//! The layout engine finds the fit. The popups are the children of a flex
//! column of the room's height that wraps, in [`order`]'s order: the
//! longest prefix of that order that fits whole, gaps included, is laid
//! out in the first column, and the rest wrap into a second column outside
//! the surface, never seen. The daemon only orders the children, and reads
//! where each landed ([`shown`]): in the first column and whole, it is
//! shown; anywhere else it waits.
//!
//! The one case wrapping does not settle: a first popup taller than the
//! room stays in the first column and is cut at its bottom. [`shown`]
//! counts it as waiting, and the column hides it, so that no popup is ever
//! drawn cut.

use std::collections::BTreeMap;

use deskrinch_notification_service::NotificationId;

/// How far a popup may run past the room and still count as whole: the
/// layout's rounding of fractional pixels.
const ROUNDING: f32 = 1.0;

/// The column's children's order: the critical notifications first, then
/// the others, the newest first within each. `live` is the model's order,
/// first shown first, each with whether it is critical.
#[must_use]
pub(crate) fn order(live: &[(NotificationId, bool)]) -> Vec<NotificationId> {
    let newest_first = || live.iter().rev();
    newest_first()
        .filter(|(_, critical)| *critical)
        .chain(newest_first().filter(|(_, critical)| !*critical))
        .map(|(id, _)| *id)
        .collect()
}

/// A box as the layout reported it, in the window's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Place {
    /// Its left edge.
    pub(crate) x: f32,
    /// Its top edge.
    pub(crate) y: f32,
    /// Its width.
    pub(crate) width: f32,
    /// Its height.
    pub(crate) height: f32,
}

impl Place {
    fn bottom(self) -> f32 {
        self.y + self.height
    }
}

/// What the column shows, read from the layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Fit {
    /// The notifications shown, in the column's order.
    pub(crate) shown: Vec<NotificationId>,
    /// The notifications waiting: wrapped out of the first column, cut, or
    /// not laid out yet.
    pub(crate) waiting: Vec<NotificationId>,
}

/// The fit of the popups in `order`, given where each landed (`places`,
/// those laid out so far) inside the popups' container at `room`, in the
/// column at `column`: a popup is shown when it lies in the container's
/// first column (its left edge within the first column's width of the
/// container's) and wholly within the container's height.
///
/// The one judgment the daemon makes beyond reading the layout: the count
/// line must not hold its own room. While some wait the line takes its
/// height and the column's gap from the room, so a popup that would fit
/// once the line went would wait for ever, the line staying because it
/// waits. So while some wait, when every popup together (their heights, and
/// the gap between each two) fits in the room the column would give the
/// container with no line, none waits: the line goes, and the layout then
/// puts them all in the first column. Otherwise the layout's fit stands,
/// its prefix decided with the line present: with two waiting and room for
/// one more, both wait.
///
/// That room is read from the column's box, which is there whether or not
/// the line is: from the container's top to the column's inner bottom (its
/// box less its padding, the container's distance from its top). With no
/// line it is the container's own height, so the judgment never goes
/// against the layout then. The gap between two popups is the column's
/// padding too: the style gives the padding and both gaps one spacing
/// ([`style`](crate::style)). Nothing is assumed of a column, or of a
/// popup, not laid out yet.
///
/// The sum is held to a pixel ([`ROUNDING`]) under that room, never over
/// it: declared fitting where the layout would wrap, the line would come
/// back at once and the two fits would take turns. So within that pixel a
/// popup that would just fit waits, the line staying, and stays waiting:
/// both states hold, neither turns into the other.
#[must_use]
pub(crate) fn shown(
    order: &[NotificationId],
    places: &BTreeMap<NotificationId, Place>,
    room: Place,
    column: Option<Place>,
) -> Fit {
    let mut fit = Fit::default();
    for id in order {
        let whole_in_first = places.get(id).is_some_and(|place| {
            place.height > 0.0
                && place.x < room.x + room.width
                && place.bottom() <= room.bottom() + ROUNDING
        });
        if whole_in_first {
            fit.shown.push(*id);
        } else {
            fit.waiting.push(*id);
        }
    }
    if !fit.waiting.is_empty() && all_fit_without_the_line(order, places, room, column) {
        return Fit {
            shown: order.to_vec(),
            waiting: Vec::new(),
        };
    }
    fit
}

/// Whether every popup of `order` together fits in the room the column at
/// `column` gives the container at `room` with no count line.
fn all_fit_without_the_line(
    order: &[NotificationId],
    places: &BTreeMap<NotificationId, Place>,
    room: Place,
    column: Option<Place>,
) -> bool {
    let Some(column) = column.filter(|column| column.height > 0.0 && room.y >= column.y) else {
        return false;
    };
    let padding = room.y - column.y;
    let without_line = column.bottom() - padding - room.y;
    let mut total = 0.0;
    for (at, id) in order.iter().enumerate() {
        let Some(place) = places.get(id).filter(|place| place.height > 0.0) else {
            return false;
        };
        total += place.height + if at > 0 { padding } else { 0.0 };
    }
    total + ROUNDING <= without_line
}

#[cfg(test)]
mod tests;

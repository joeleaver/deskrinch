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
/// those laid out so far) inside the popups' container at `room`: a popup
/// is shown when it lies in the container's first column (its left edge
/// within the first column's width of the container's) and wholly within
/// the container's height.
#[must_use]
pub(crate) fn shown(
    order: &[NotificationId],
    places: &BTreeMap<NotificationId, Place>,
    room: Place,
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
    fit
}

#[cfg(test)]
mod tests;

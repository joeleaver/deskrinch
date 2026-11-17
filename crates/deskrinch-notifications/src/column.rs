//! The column's state between its popups and its root: where each popup
//! landed, the fit read from that ([`fit`]), and why each
//! notification's expiry is held ([`pauses`](crate::pauses)).
//!
//! Each popup reports its box after each layout ([`Column::place`]); the
//! content reports the popups' container, the room, with the column around
//! it ([`Column::room`]). The fit
//! follows both. When it changes, the waiting ones are paused and the shown
//! ones resumed (unless the pointer holds them), the count line follows,
//! and one line is logged at info level ([`fit_line`]).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use deskrinch_notification_service::{Handle, NotificationId};
use rinch::prelude::{Memo, Signal};
use rinch::reactive::{Effect, untracked};

use crate::fit::{self, Fit, Place};
use crate::pauses::{Pauses, Reason, Tell};

/// The line logged when the fit changes, for `shown` shown and `waiting`
/// waiting.
pub(crate) fn fit_line(shown: usize, waiting: usize) -> String {
    format!("showing {shown}, {waiting} waiting")
}

/// The column's state; cheap to clone, one for the root and each popup.
#[derive(Clone)]
pub(crate) struct Column {
    handle: Handle,
    pauses: Rc<RefCell<Pauses>>,
    order: Memo<Vec<NotificationId>>,
    places: Signal<BTreeMap<NotificationId, Place>>,
    room: Signal<Option<Room>>,
    fit: Memo<Fit>,
}

/// Where the layout put the popups' container, and the column around it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Room {
    /// The popups' container.
    pub(crate) container: Place,
    /// The column: its padding, the container, and the count line while it
    /// shows.
    pub(crate) column: Place,
}

impl Column {
    /// The column of the notifications in `order` (the children's order,
    /// [`fit::order`]), acting through `handle`.
    pub(crate) fn new(handle: Handle, order: Memo<Vec<NotificationId>>) -> Self {
        let places: Signal<BTreeMap<NotificationId, Place>> = Signal::new(BTreeMap::new());
        let room: Signal<Option<Room>> = Signal::new(None);
        let fit = Memo::new(move || {
            let order = order.get();
            match room.get() {
                Some(room) => places
                    .with(|places| fit::shown(&order, places, room.container, Some(room.column))),
                // Before the first layout nothing is shown or waiting yet.
                None => Fit::default(),
            }
        });
        let column = Self {
            handle,
            pauses: Rc::default(),
            order,
            places,
            room,
            fit,
        };
        let follower = column.clone();
        Effect::new(move || {
            let fit = fit.get();
            let order = order.get();
            untracked(|| follower.follow(&order, &fit));
        });
        column
    }

    /// The popups' container and the column are at `room`.
    pub(crate) fn room(&self, room: Room) {
        self.room.set_if_changed(Some(room));
    }

    /// The popup of `id` is at `place`. A box not laid out yet (no height)
    /// is not a place. The places of notifications gone are dropped here.
    pub(crate) fn place(&self, id: NotificationId, place: Place) {
        if place.height <= 0.0 {
            return;
        }
        untracked(|| {
            let order = self.order.get();
            let stale = self.places.with(|places| {
                places.get(&id) != Some(&place) || places.keys().any(|k| !order.contains(k))
            });
            if stale {
                self.places.update(|places| {
                    places.retain(|known, _| order.contains(known));
                    places.insert(id, place);
                });
            }
        });
    }

    /// Whether `id` waits (is not shown): its popup is hidden and takes no
    /// input.
    pub(crate) fn waiting(&self, id: NotificationId) -> bool {
        self.fit.get().waiting.contains(&id)
    }

    /// How many wait.
    pub(crate) fn waiting_count(&self) -> usize {
        self.fit.get().waiting.len()
    }

    /// The pointer is over `id`'s popup (`on`) or has left it.
    pub(crate) fn pointer(&self, id: NotificationId, on: bool) {
        let tell = self.pauses.borrow_mut().set(id, Reason::Pointer, on);
        self.tell(id, tell);
    }

    /// The fit is now `fit`, of the live `order`: the reasons follow, the
    /// gone are forgotten, and the line is logged.
    fn follow(&self, order: &[NotificationId], fit: &Fit) {
        self.pauses.borrow_mut().retain(order);
        for (ids, waiting) in [(&fit.waiting, true), (&fit.shown, false)] {
            for id in ids {
                let tell = self.pauses.borrow_mut().set(*id, Reason::Waiting, waiting);
                self.tell(*id, tell);
            }
        }
        if self.room.get().is_some() {
            tracing::info!("{}", fit_line(fit.shown.len(), fit.waiting.len()));
        }
    }

    fn tell(&self, id: NotificationId, tell: Tell) {
        // A notification that closed meanwhile has nothing to pause.
        let _gone = match tell {
            Tell::Pause => self.handle.pause(id),
            Tell::Resume => self.handle.resume(id),
            Tell::Nothing => Ok(()),
        };
    }
}

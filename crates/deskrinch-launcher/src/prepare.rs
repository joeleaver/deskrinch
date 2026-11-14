//! When the hidden launcher prepares its window for its next show: see
//! [`Preparation`].

use std::sync::Arc;

use deskrinch_icons::Index;
use deskrinch_theme::Scheme;

use crate::Row;

/// What the launcher's next show draws, as far as it changes while hidden.
#[derive(Debug, Clone)]
pub struct Drawn {
    /// The results an empty query shows ([`Model::frequent`](crate::Model::frequent)).
    pub rows: Vec<Row>,
    /// The icon theme's index, once there is one; compared by identity, as
    /// each one built is new.
    pub icons: Option<Arc<Index>>,
    /// Dark or light.
    pub look: Scheme,
}

impl PartialEq for Drawn {
    fn eq(&self, other: &Self) -> bool {
        self.changes(Some(other)).is_empty()
    }
}

impl Drawn {
    /// What differs from `before`, for the log: everything when there is
    /// none.
    #[must_use]
    pub fn changes(&self, before: Option<&Self>) -> Vec<&'static str> {
        let Some(before) = before else {
            return vec!["everything (not prepared before)"];
        };
        let icons = match (&self.icons, &before.icons) {
            (Some(now), Some(then)) => !Arc::ptr_eq(now, then),
            (now, then) => now.is_some() != then.is_some(),
        };
        [
            ("the results", self.rows != before.rows),
            ("the icons", icons),
            ("the look", self.look != before.look),
        ]
        .into_iter()
        .filter_map(|(what, changed)| changed.then_some(what))
        .collect()
    }
}

/// What the launcher last prepared, and whether to prepare again.
///
/// rinch styles, lays out and paints a window, and on the GPU makes its
/// renderer, only for a frame it presents; a hidden window presents none,
/// so the first show would pay for all of it. Asked to prepare
/// (`rinch::windows::prepare_current_window`), rinch does that work for the
/// hidden window without presenting anything, and keeps what it made.
///
/// What a show draws, as far as it can change while the launcher is hidden,
/// is [`Drawn`]: the results an empty query shows (the frequent
/// applications, which the installed applications and the record of starts
/// make), the icon theme's index their icons come from, and the look. The
/// launcher prepares whenever that changes while it is hidden, the first
/// time included ([`wants`](Self::wants)), and never while it is shown,
/// when its frames are its preparation. A change made while it is shown is
/// prepared with the next change made while it is hidden.
///
/// The pictures need nothing more: rinch loads an icon's file on a worker,
/// and a prepared window that is still hidden prepares itself again when
/// one lands.
#[derive(Debug, Default)]
pub struct Preparation {
    prepared: Option<Drawn>,
}

impl Preparation {
    /// The next show now draws `drawn`, and the launcher is `shown` or not:
    /// what changed since the last preparation, when it should prepare now
    /// (hidden, and something changed); nothing otherwise.
    pub fn wants(&mut self, drawn: Drawn, shown: bool) -> Vec<&'static str> {
        if shown {
            return Vec::new();
        }
        let changes = drawn.changes(self.prepared.as_ref());
        if !changes.is_empty() {
            self.prepared = Some(drawn);
        }
        changes
    }
}

#[cfg(test)]
#[path = "prepare_tests.rs"]
mod tests;

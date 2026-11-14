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
    /// What differs from `before`: everything when there is none.
    #[must_use]
    pub fn changes(&self, before: Option<&Self>) -> Vec<Change> {
        let Some(before) = before else {
            return vec![Change::Everything];
        };
        let icons = match (&self.icons, &before.icons) {
            (Some(now), Some(then)) => !Arc::ptr_eq(now, then),
            (now, then) => now.is_some() != then.is_some(),
        };
        [
            (Change::Results, self.rows != before.rows),
            (Change::Icons, icons),
            (Change::Look, self.look != before.look),
        ]
        .into_iter()
        .filter_map(|(what, changed)| changed.then_some(what))
        .collect()
    }
}

/// Why the launcher prepares, as the log names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Nothing was prepared before.
    Everything,
    /// It was just hidden: its last frame was the shown one's.
    Hidden,
    /// The results an empty query shows changed.
    Results,
    /// The icon theme's index changed.
    Icons,
    /// Dark or light changed.
    Look,
}

impl Change {
    /// Its words in the log's line ([`preparing`]).
    #[must_use]
    pub fn says(self) -> &'static str {
        match self {
            Self::Everything => "nothing was prepared before",
            Self::Hidden => "it was hidden",
            Self::Results => "the results changed",
            Self::Icons => "the icons changed",
            Self::Look => "the look changed",
        }
    }
}

/// How the launcher's line saying it prepares begins ([`preparing`]).
pub const PREPARING: &str = "preparing the hidden window for its next show:";

/// The line the launcher writes, at info, each time it asks rinch to
/// prepare its hidden window, saying why: one home for its wording, which
/// the launcher logs with and its tests read.
#[must_use]
pub fn preparing(changes: &[Change]) -> String {
    let named: Vec<&str> = changes.iter().map(|change| change.says()).collect();
    format!("{PREPARING} {}", named.join("; "))
}

/// What the launcher last prepared, and whether to prepare again.
///
/// rinch styles, lays out and paints a window, and on the GPU makes its
/// renderer, only for a frame it presents; a hidden window presents none,
/// so the first show would pay for all of it. Asked to prepare
/// (`rinch::windows::prepare_current_window`), rinch does that work for the
/// hidden window without presenting anything, and keeps what it made.
///
/// The rule: while the launcher is hidden, the frame it last painted is
/// what its next show draws. So it prepares at start, at every hide, and
/// whenever what the next show draws ([`Drawn`]) changes while it is
/// hidden; never while it is shown, when its frames are its preparation.
///
/// - At a hide the last painted frame is the shown one's: the query and its
///   results, which the hide clears back to the frequent applications. The
///   launcher prepares at every hide, without asking whether the hide
///   changed what is drawn: one with nothing changed costs about 17 thousand
///   instructions with the software renderer and 21 thousand with the GPU
///   one (measured with `cargo xtask measure --summon --instructions`,
///   against 132 and 216 thousand for the hide itself), too little to make
///   the rule conditional.
/// - While hidden, [`Drawn`] changes when the frequent applications do (the
///   installed applications, the record of starts and `[general]` make
///   them), when the icon theme's index does, and when the look does.
/// - A change made while it is shown needs nothing more: the hide that
///   follows prepares.
///
/// The pictures need nothing either: rinch loads an icon's file on a
/// worker, and a prepared window that is still hidden prepares itself
/// again when one lands.
#[derive(Debug, Default)]
pub struct Preparation {
    prepared: Option<Drawn>,
    /// Whether it was shown when last asked.
    shown: bool,
}

impl Preparation {
    /// The next show now draws `drawn`, and the launcher is `shown` or not:
    /// why it should prepare now (hidden, and just hidden or something
    /// changed since the last preparation); nothing otherwise.
    pub fn wants(&mut self, drawn: Drawn, shown: bool) -> Vec<Change> {
        let hidden_now = self.shown && !shown;
        self.shown = shown;
        if shown {
            return Vec::new();
        }
        let mut changes = drawn.changes(self.prepared.as_ref());
        if hidden_now && changes != [Change::Everything] {
            changes.insert(0, Change::Hidden);
        }
        if !changes.is_empty() {
            self.prepared = Some(drawn);
        }
        changes
    }
}

#[cfg(test)]
#[path = "prepare_tests.rs"]
mod tests;

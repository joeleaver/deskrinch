//! The launcher's model: the query, the ranked results and the selection,
//! and what each input does to them.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use deskrinch_desktop_entries::{DesktopEntry, DesktopId, Icon, Locale, search};
use deskrinch_gestures::Click;
use deskrinch_start_record::Record;

/// One result: an application to show and start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The application, to start it by.
    pub id: DesktopId,
    /// Its name, in the user's language.
    pub name: String,
    /// Its icon as the entry names it (an icon-theme name or a file), in
    /// the user's language; unresolved.
    pub icon: Option<Icon>,
}

/// What the user does to the launcher while it is shown.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Input {
    /// The selection moves to the result above.
    Up,
    /// The selection moves to the result below.
    Down,
    /// The selected application is started.
    Enter,
    /// The launcher is dismissed.
    Escape,
    /// The launcher's surface lost the keyboard.
    KeyboardLost,
    /// The primary pointer button went down: on the result with this id,
    /// or (`None`) anywhere else in the launcher.
    Press(Option<DesktopId>),
    /// The primary pointer button came up: on the result with this id, or
    /// (`None`) anywhere else. On the result it went down on, that is a
    /// click, which starts it.
    Release(Option<DesktopId>),
}

/// What the process does after an [`Input`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    /// Nothing beyond showing the model as it now is.
    Stay,
    /// Start this application, then hide.
    Launch(DesktopId),
    /// Hide.
    Hide,
}

/// What the launcher shows: the query, the results that match it, best
/// first, and which one is selected. The rules are in the crate
/// documentation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Model {
    query: String,
    rows: Vec<Row>,
    /// An index into `rows`; `None` exactly when there are no rows.
    selected: Option<usize>,
    /// The result the primary pointer button went down on, while it is
    /// down: a release on the same result is a click.
    pressed: Click<DesktopId>,
    /// The first of the [`ROWS`] rows drawn: it moves only when the
    /// selection would leave them.
    first: usize,
    /// Whether an application was started since the last [`clear`](Self::clear):
    /// one start per show.
    launched: bool,
    /// The rows an empty query shows: the applications used most.
    frequent: Vec<Row>,
}

/// How many results are drawn at once.
pub const ROWS: usize = 8;

/// How many of the applications used most an empty query shows.
pub const FREQUENT: usize = 5;

impl Model {
    /// An empty launcher: no query, no results.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What the user typed.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The applications that match the query, best first.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The applications an empty query shows from the next
    /// [`clear`](Self::clear): what a hidden launcher's next show draws.
    #[must_use]
    pub fn frequent(&self) -> &[Row] {
        &self.frequent
    }

    /// The index of the selected row; `None` when there are no rows.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The selected row.
    #[must_use]
    pub fn selected_row(&self) -> Option<&Row> {
        self.selected.and_then(|index| self.rows.get(index))
    }

    /// Sets the query to `query` and searches `entries` (the applications
    /// shown in menus) for it, in the user's `locales`; an empty query's
    /// results are the [frequent](Self::set_frequent) applications. The
    /// first result is selected.
    pub fn set_query<'a>(
        &mut self,
        query: &str,
        entries: impl IntoIterator<Item = &'a DesktopEntry>,
        locales: &[Locale],
    ) {
        query.clone_into(&mut self.query);
        self.rows = if self.query.is_empty() {
            self.frequent.clone()
        } else {
            rows(entries, &self.query, locales)
        };
        self.selected = (!self.rows.is_empty()).then_some(0);
        self.first = 0;
    }

    /// Sets the applications an empty query shows (from [`frequent`]). They
    /// are shown from the next [`clear`](Self::clear), or the next time the
    /// query is emptied: never in place of what is shown now.
    pub fn set_frequent(&mut self, rows: Vec<Row>) {
        self.frequent = rows;
    }

    /// Searches `entries` again for the same query, when the applications
    /// changed. The selection stays on the same application if it is still
    /// a result, and is the first result otherwise. With an empty query,
    /// the frequent applications shown stay, less any no longer in
    /// `entries`.
    pub fn refresh<'a>(
        &mut self,
        entries: impl IntoIterator<Item = &'a DesktopEntry>,
        locales: &[Locale],
    ) {
        let selected = self.selected_row().map(|row| row.id.clone());
        self.rows = if self.query.is_empty() {
            let installed: BTreeSet<&DesktopId> = entries.into_iter().map(|e| &e.id).collect();
            let mut kept = std::mem::take(&mut self.rows);
            kept.retain(|row| installed.contains(&row.id));
            kept
        } else {
            rows(entries, &self.query, locales)
        };
        let kept = selected.and_then(|id| self.rows.iter().position(|row| row.id == id));
        self.selected = kept.or((!self.rows.is_empty()).then_some(0));
        self.first = self.first.min(self.rows.len().saturating_sub(ROWS));
        self.keep_in_sight();
    }

    /// What `input` does: the selection moved, or the action to take.
    pub fn input(&mut self, input: Input) -> Action {
        match input {
            Input::Up => {
                self.selected = self.selected.map(|index| index.saturating_sub(1));
                self.keep_in_sight();
                Action::Stay
            }
            Input::Down => {
                let last = self.rows.len().saturating_sub(1);
                self.selected = self.selected.map(|index| (index + 1).min(last));
                self.keep_in_sight();
                Action::Stay
            }
            Input::Enter => match self.selected_row() {
                Some(row) => {
                    let id = row.id.clone();
                    self.launch(id)
                }
                None => Action::Stay,
            },
            Input::Escape | Input::KeyboardLost => Action::Hide,
            Input::Press(on) => {
                self.pressed.press(on, ());
                Action::Stay
            }
            Input::Release(on) => match self.pressed.release(on, ()) {
                Some(clicked) if self.rows.iter().any(|row| row.id == clicked) => {
                    self.launch(clicked)
                }
                _ => Action::Stay,
            },
        }
    }

    /// Which rows are drawn: [`ROWS`] of them from the first drawn, which
    /// moves only as far as keeps the selection among them.
    #[must_use]
    pub fn visible(&self) -> Range<usize> {
        self.first..(self.first + ROWS).min(self.rows.len())
    }

    /// Starts `id`, unless one was started since the last clear.
    fn launch(&mut self, id: DesktopId) -> Action {
        if self.launched {
            return Action::Stay;
        }
        self.launched = true;
        Action::Launch(id)
    }

    /// Moves the first drawn row only as far as brings the selection back
    /// among the drawn rows.
    fn keep_in_sight(&mut self) {
        let Some(selected) = self.selected else {
            self.first = 0;
            return;
        };
        if selected < self.first {
            self.first = selected;
        } else if selected >= self.first + ROWS {
            self.first = selected + 1 - ROWS;
        }
    }

    /// Empties the launcher for when it hides: no query, and the frequent
    /// applications as its results, the first selected.
    pub fn clear(&mut self) {
        let frequent = std::mem::take(&mut self.frequent);
        *self = Self {
            selected: (!frequent.is_empty()).then_some(0),
            rows: frequent.clone(),
            frequent,
            ..Self::default()
        };
    }
}

/// The [`FREQUENT`] applications of `entries` (those shown in menus) that
/// `record` ranks best, as rows in the user's `locales`. One no longer
/// installed, or no longer shown in menus, is passed over.
#[must_use]
pub fn frequent(record: &Record, entries: &[DesktopEntry], locales: &[Locale]) -> Vec<Row> {
    if record.is_empty() {
        return Vec::new();
    }
    let by_id: BTreeMap<&DesktopId, &DesktopEntry> =
        entries.iter().map(|entry| (&entry.id, entry)).collect();
    record
        .top(FREQUENT, |id| by_id.contains_key(id))
        .iter()
        .filter_map(|id| by_id.get(id))
        .map(|entry| row(entry, locales))
        .collect()
}

/// The rows for `query`, best first.
fn rows<'a>(
    entries: impl IntoIterator<Item = &'a DesktopEntry>,
    query: &str,
    locales: &[Locale],
) -> Vec<Row> {
    search(entries, query, locales)
        .into_iter()
        .map(|found| row(found.entry, locales))
        .collect()
}

/// The row of `entry`, in the user's `locales`.
fn row(entry: &DesktopEntry, locales: &[Locale]) -> Row {
    Row {
        id: entry.id.clone(),
        name: entry.name.get(locales).clone(),
        icon: entry.icon.as_ref().map(|icon| icon.get(locales).clone()),
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "frequent_tests.rs"]
mod frequent_tests;

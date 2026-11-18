//! [`Switcher`]: the workspaces as the switcher keeps them, and the runs of
//! buttons they make.

use std::collections::BTreeSet;

/// What the switcher uses of a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<Id, G> {
    /// Its identity.
    pub id: Id,
    /// Whether it is the one shown on its group's outputs.
    pub active: bool,
    /// Whether the compositor asks a switcher not to show it.
    pub hidden: bool,
    /// Its group, where the compositor has said.
    pub group: Option<G>,
    /// Its place in the compositor's layout, one coordinate per dimension;
    /// empty where the compositor gives none.
    pub coordinates: Vec<u32>,
    /// Whether the compositor allows activating it.
    pub activatable: bool,
}

/// A change to the workspaces the switcher follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change<Id, G> {
    /// A workspace appeared, or changed: its whole new state.
    Workspace(Entry<Id, G>),
    /// A workspace went away.
    Removed(Id),
    /// A group appeared, or its outputs changed: the names of the outputs
    /// it is on.
    Group {
        /// Its identity.
        id: G,
        /// The names of its outputs.
        outputs: Vec<String>,
    },
    /// A group went away.
    GroupRemoved(G),
    /// The compositor stopped reporting workspaces: the switcher is empty.
    Ended,
}

/// One workspace's button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button<Id> {
    /// The workspace.
    pub id: Id,
    /// What it shows: its place in its run, from 1.
    pub label: String,
    /// Whether it is the workspace shown on its run's outputs.
    pub current: bool,
    /// Whether a click asks to activate it.
    pub activatable: bool,
}

/// The buttons of one group of workspaces, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run<Id> {
    /// Its buttons.
    pub buttons: Vec<Button<Id>>,
    /// The names of its group's outputs: none for the run of the workspaces
    /// in no known group, and for a group that names none. Which bars show
    /// it ([`crate::runs_on`]).
    pub outputs: BTreeSet<String>,
}

/// The workspaces and groups as last reported.
#[derive(Debug, Clone)]
pub struct Switcher<Id, G> {
    /// Every workspace, in the order first reported.
    entries: Vec<Entry<Id, G>>,
    /// Every group, with its outputs' names, sorted.
    groups: Vec<(G, Vec<String>)>,
}

impl<Id, G> Default for Switcher<Id, G> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            groups: Vec::new(),
        }
    }
}

impl<Id: Copy + PartialEq, G: Copy + PartialEq> Switcher<Id, G> {
    /// A switcher with nothing reported yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies `change`.
    pub fn apply(&mut self, change: Change<Id, G>) {
        match change {
            Change::Workspace(entry) => {
                match self.entries.iter_mut().find(|known| known.id == entry.id) {
                    Some(known) => *known = entry,
                    None => self.entries.push(entry),
                }
            }
            Change::Removed(id) => self.entries.retain(|known| known.id != id),
            Change::Group { id, mut outputs } => {
                outputs.sort();
                match self.groups.iter_mut().find(|(known, _)| *known == id) {
                    Some((_, known)) => *known = outputs,
                    None => self.groups.push((id, outputs)),
                }
            }
            Change::GroupRemoved(id) => self.groups.retain(|(known, _)| *known != id),
            Change::Ended => {
                self.entries.clear();
                self.groups.clear();
            }
        }
    }

    /// Whether a click on `id`'s button asks to activate it: a workspace
    /// listed, shown and activatable.
    #[must_use]
    pub fn activatable(&self, id: Id) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.id == id && !entry.hidden && entry.activatable)
    }

    /// The runs of buttons: one per group with a workspace shown, ordered
    /// by their outputs' names, then the workspaces of no known group.
    #[must_use]
    pub fn runs(&self) -> Vec<Run<Id>> {
        let mut groups: Vec<&(G, Vec<String>)> = self.groups.iter().collect();
        groups.sort_by(|(_, a), (_, b)| (a.is_empty(), a.first()).cmp(&(b.is_empty(), b.first())));
        let known = |group: Option<G>| group.is_some_and(|g| groups.iter().any(|(k, _)| *k == g));
        groups
            .iter()
            .map(|(id, outputs)| self.run(|entry| entry.group == Some(*id), outputs))
            .chain(std::iter::once(self.run(|entry| !known(entry.group), &[])))
            .filter(|run| !run.buttons.is_empty())
            .collect()
    }

    /// The run of the shown workspaces `member` picks, on the outputs named
    /// `outputs`, in the compositor's layout where every one of them has a
    /// place in it, else in the order reported.
    fn run(&self, member: impl Fn(&Entry<Id, G>) -> bool, outputs: &[String]) -> Run<Id> {
        let mut entries: Vec<&Entry<Id, G>> = self
            .entries
            .iter()
            .filter(|entry| !entry.hidden && member(entry))
            .collect();
        if entries.iter().all(|entry| !entry.coordinates.is_empty()) {
            entries.sort_by(|a, b| a.coordinates.cmp(&b.coordinates));
        }
        let buttons = entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| Button {
                id: entry.id,
                label: (index + 1).to_string(),
                current: entry.active,
                activatable: entry.activatable,
            })
            .collect();
        Run {
            buttons,
            outputs: outputs.iter().cloned().collect(),
        }
    }
}

#[cfg(test)]
#[path = "switcher_tests.rs"]
mod tests;

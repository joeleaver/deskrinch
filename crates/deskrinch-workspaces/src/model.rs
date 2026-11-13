//! The workspaces and groups as the protocol has described them: changes
//! are staged as they arrive and delivered together when the protocol says
//! a batch is complete, each workspace and group in its whole new state.
//!
//! Everything a compositor sends is bounded here: at most
//! [`MAX_WORKSPACES`] workspaces and [`MAX_GROUPS`] groups are listed (more
//! are ignored), names are cleaned of control characters and cut to
//! [`MAX_NAME_BYTES`], and coordinates of more than [`MAX_DIMENSIONS`]
//! dimensions are dropped.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use deskrinch_untrusted::text::{self, Lines};

use crate::capabilities::Actions;
use crate::workspace::{Event, Group, GroupId, Workspace, WorkspaceId, WorkspaceState};

/// The most workspaces listed.
pub(crate) const MAX_WORKSPACES: usize = 256;
/// The most groups listed.
pub(crate) const MAX_GROUPS: usize = 64;
/// The longest name kept, in bytes.
pub(crate) const MAX_NAME_BYTES: usize = 256;
/// The most dimensions a workspace's coordinates may have.
pub(crate) const MAX_DIMENSIONS: usize = 8;

/// One staged change to a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    /// Its name, as the compositor sent it.
    Name(String),
    State(WorkspaceState),
    Group(Option<GroupId>),
    /// Its coordinates, as the compositor sent them.
    Coordinates(Vec<u32>),
    Actions(Actions),
}

/// A listed workspace: as it is now, and as last delivered.
#[derive(Debug)]
struct Entry {
    current: Workspace,
    delivered: Option<Workspace>,
    removed: bool,
    /// Whether its protocol has described it whole; until then it is not
    /// delivered.
    ready: bool,
}

/// A listed group: as it is now, and as last delivered.
#[derive(Debug)]
struct GroupEntry {
    current: Group,
    delivered: Option<Group>,
    removed: bool,
}

/// The workspaces and groups of one connection.
#[derive(Debug, Default)]
pub(crate) struct WorkspaceList {
    next: u64,
    order: Vec<WorkspaceId>,
    entries: HashMap<WorkspaceId, Entry>,
    groups: BTreeMap<GroupId, GroupEntry>,
}

impl WorkspaceList {
    /// Lists a new workspace, empty until its changes are committed; `None`
    /// when [`MAX_WORKSPACES`] are listed.
    pub(crate) fn add(&mut self) -> Option<WorkspaceId> {
        self.add_entry(true)
    }

    /// Lists a new workspace that is not delivered until [`ready`](Self::ready)
    /// says its protocol has described it whole (KWin names a desktop only
    /// after announcing it).
    pub(crate) fn add_unready(&mut self) -> Option<WorkspaceId> {
        self.add_entry(false)
    }

    /// Workspace `id` is described whole: its changes are delivered from
    /// the next commit on.
    pub(crate) fn ready(&mut self, id: WorkspaceId) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.ready = true;
        }
    }

    fn add_entry(&mut self, ready: bool) -> Option<WorkspaceId> {
        if self.entries.len() >= MAX_WORKSPACES {
            return None;
        }
        self.next += 1;
        let id = WorkspaceId(self.next);
        let current = Workspace {
            id,
            name: String::new(),
            state: WorkspaceState::empty(),
            group: None,
            coordinates: Vec::new(),
            actions: Actions::empty(),
        };
        self.entries.insert(
            id,
            Entry {
                current,
                delivered: None,
                removed: false,
                ready,
            },
        );
        self.order.push(id);
        Some(id)
    }

    /// Lists a new group; `None` when [`MAX_GROUPS`] are listed.
    pub(crate) fn add_group(&mut self) -> Option<GroupId> {
        if self.groups.len() >= MAX_GROUPS {
            return None;
        }
        self.next += 1;
        let id = GroupId(self.next);
        let current = Group {
            id,
            outputs: BTreeSet::new(),
        };
        self.groups.insert(
            id,
            GroupEntry {
                current,
                delivered: None,
                removed: false,
            },
        );
        Some(id)
    }

    /// Stages `change` to workspace `id`.
    pub(crate) fn stage(&mut self, id: WorkspaceId, change: Change) {
        let Some(entry) = self.entries.get_mut(&id) else {
            return;
        };
        let workspace = &mut entry.current;
        match change {
            Change::Name(name) => workspace.name = text::clean(&name, MAX_NAME_BYTES, Lines::Join),
            Change::State(state) => workspace.state = state,
            Change::Group(group) => {
                workspace.group = group.filter(|group| self.groups.contains_key(group));
            }
            Change::Coordinates(coordinates) => {
                workspace.coordinates = if coordinates.len() <= MAX_DIMENSIONS {
                    coordinates
                } else {
                    Vec::new()
                };
            }
            Change::Actions(actions) => workspace.actions = actions,
        }
    }

    /// Stages the outputs of group `id`.
    pub(crate) fn stage_outputs(&mut self, id: GroupId, outputs: BTreeSet<String>) {
        if let Some(group) = self.groups.get_mut(&id) {
            group.current.outputs = outputs;
        }
    }

    /// Stages the removal of workspace `id`: it leaves the order at once,
    /// and is reported removed at the commit.
    pub(crate) fn remove(&mut self, id: WorkspaceId) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.removed = true;
            self.order.retain(|&listed| listed != id);
        }
    }

    /// Stages the removal of group `id`; its workspaces leave it.
    pub(crate) fn remove_group(&mut self, id: GroupId) {
        let Some(group) = self.groups.get_mut(&id) else {
            return;
        };
        group.removed = true;
        for entry in self.entries.values_mut() {
            if entry.current.group == Some(id) {
                entry.current.group = None;
            }
        }
    }

    /// Moves workspace `id` to `index` in the order (clamped to the end).
    pub(crate) fn move_to(&mut self, id: WorkspaceId, index: usize) {
        let Some(from) = self.order.iter().position(|&listed| listed == id) else {
            return;
        };
        self.order.remove(from);
        self.order.insert(index.min(self.order.len()), id);
    }

    /// The workspaces in order, without those whose removal is staged.
    pub(crate) fn order(&self) -> &[WorkspaceId] {
        &self.order
    }

    /// Delivers what changed since the last commit: groups that appeared or
    /// changed, workspaces that appeared or changed (in order), workspaces
    /// removed, then groups removed.
    pub(crate) fn commit(&mut self) -> Vec<Event> {
        let mut events = self.changed_groups(true);
        for id in &self.order {
            let Some(entry) = self.entries.get_mut(id) else {
                continue;
            };
            if entry.removed || !entry.ready || entry.delivered.as_ref() == Some(&entry.current) {
                continue;
            }
            let event = if entry.delivered.is_some() {
                Event::Changed(entry.current.clone())
            } else {
                Event::Added(entry.current.clone())
            };
            entry.delivered = Some(entry.current.clone());
            events.push(event);
        }
        let mut removed: Vec<WorkspaceId> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.removed)
            .map(|(&id, _)| id)
            .collect();
        removed.sort_unstable();
        for id in removed {
            if let Some(entry) = self.entries.remove(&id)
                && entry.delivered.is_some()
            {
                events.push(Event::Removed(id));
            }
        }
        let gone: Vec<GroupId> = self
            .groups
            .iter()
            .filter(|(_, group)| group.removed)
            .map(|(&id, _)| id)
            .collect();
        for id in gone {
            if let Some(group) = self.groups.remove(&id)
                && group.delivered.is_some()
            {
                events.push(Event::GroupRemoved(id));
            }
        }
        events
    }

    /// Delivers the groups already delivered whose outputs changed since,
    /// alone: what an output's name or going away changes between the
    /// protocol's batches. Every staged workspace change and removal, and
    /// every group not yet delivered or whose removal is staged, waits for
    /// the next [`commit`](Self::commit).
    pub(crate) fn commit_groups(&mut self) -> Vec<Event> {
        self.changed_groups(false)
    }

    /// Marks delivered, and lists, the groups that differ from what was
    /// delivered, among those not being removed; groups never delivered only
    /// if `new` says so.
    fn changed_groups(&mut self, new: bool) -> Vec<Event> {
        let mut events = Vec::new();
        for group in self.groups.values_mut().filter(|group| !group.removed) {
            let changed = match &group.delivered {
                Some(delivered) => *delivered != group.current,
                None => new,
            };
            if changed {
                group.delivered = Some(group.current.clone());
                events.push(Event::GroupChanged(group.current.clone()));
            }
        }
        events
    }

    /// The workspaces as last delivered, in order.
    pub(crate) fn snapshot(&self) -> Vec<Workspace> {
        self.order
            .iter()
            .filter_map(|id| self.entries.get(id)?.delivered.clone())
            .collect()
    }

    /// The groups as last delivered.
    pub(crate) fn groups(&self) -> Vec<Group> {
        self.groups
            .values()
            .filter_map(|group| group.delivered.clone())
            .collect()
    }

    /// The workspace `id` as last delivered.
    pub(crate) fn get(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.entries.get(&id)?.delivered.as_ref()
    }

    /// Forgets everything.
    pub(crate) fn clear(&mut self) {
        self.order.clear();
        self.entries.clear();
        self.groups.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(events: &[Event]) -> Vec<String> {
        events
            .iter()
            .map(|event| match event {
                Event::Added(workspace) => format!("added {}", workspace.name),
                Event::Changed(workspace) => format!("changed {}", workspace.name),
                Event::Removed(id) => format!("removed {}", id.0),
                Event::GroupChanged(group) => format!("group {}", group.outputs.len()),
                Event::GroupRemoved(id) => format!("group removed {}", id.0),
                Event::Ended(_) => "ended".to_owned(),
            })
            .collect()
    }

    #[test]
    fn a_batch_is_delivered_whole_and_only_once() {
        let mut list = WorkspaceList::default();
        let one = list.add().unwrap();
        let two = list.add().unwrap();
        list.stage(one, Change::Name("One".into()));
        list.stage(one, Change::State(WorkspaceState::ACTIVE));
        list.stage(two, Change::Name("Two".into()));
        assert!(list.snapshot().is_empty(), "nothing before the commit");
        assert_eq!(names(&list.commit()), ["added One", "added Two"]);
        assert!(list.commit().is_empty(), "nothing changed since");
        list.stage(one, Change::State(WorkspaceState::empty()));
        list.stage(two, Change::State(WorkspaceState::ACTIVE));
        assert_eq!(names(&list.commit()), ["changed One", "changed Two"]);
        assert_eq!(list.get(two).unwrap().state, WorkspaceState::ACTIVE);
    }

    #[test]
    fn removals_follow_changes_and_unannounced_ones_are_silent() {
        let mut list = WorkspaceList::default();
        let kept = list.add().unwrap();
        let gone = list.add().unwrap();
        list.stage(kept, Change::Name("Kept".into()));
        list.commit();
        list.remove(gone);
        list.stage(kept, Change::Name("Renamed".into()));
        let brief = list.add().unwrap();
        list.remove(brief);
        let events = names(&list.commit());
        assert_eq!(
            events,
            ["changed Renamed", format!("removed {}", gone.0).as_str()]
        );
        assert_eq!(list.order(), [kept]);
    }

    #[test]
    fn groups_come_first_and_their_removal_last() {
        let mut list = WorkspaceList::default();
        let group = list.add_group().unwrap();
        let workspace = list.add().unwrap();
        list.stage_outputs(group, BTreeSet::from(["HEADLESS-1".to_owned()]));
        list.stage(workspace, Change::Group(Some(group)));
        assert_eq!(names(&list.commit()), ["group 1", "added "]);
        assert_eq!(list.get(workspace).unwrap().group, Some(group));
        list.remove_group(group);
        let events = names(&list.commit());
        assert_eq!(
            events,
            ["changed ", format!("group removed {}", group.0).as_str()]
        );
        assert_eq!(list.get(workspace).unwrap().group, None);
        assert_eq!(list.groups(), []);
    }

    #[test]
    fn an_outputs_only_delivery_leaves_the_batch_for_its_commit() {
        let mut list = WorkspaceList::default();
        let group = list.add_group().unwrap();
        let workspace = list.add().unwrap();
        list.stage_outputs(group, BTreeSet::from(["HEADLESS-1".to_owned()]));
        list.stage(workspace, Change::Name("One".into()));
        list.stage(workspace, Change::Group(Some(group)));
        list.commit();
        // A batch is open: a workspace changes and another appears, and a
        // new group too, when an output is renamed.
        list.stage(workspace, Change::Name("Renamed".into()));
        list.stage(workspace, Change::Group(None));
        let added = list.add().unwrap();
        list.stage(added, Change::Name("Two".into()));
        let new_group = list.add_group().unwrap();
        list.stage_outputs(group, BTreeSet::from(["HEADLESS-9".to_owned()]));
        let events = list.commit_groups();
        assert_eq!(names(&events), ["group 1"], "the group alone");
        assert!(
            matches!(&events[0], Event::GroupChanged(changed) if changed.id == group),
            "{events:?}"
        );
        assert_eq!(list.get(workspace).unwrap().name, "One");
        assert_eq!(list.get(workspace).unwrap().group, Some(group));
        assert_eq!(list.get(added), None);
        assert!(list.commit_groups().is_empty(), "delivered once");
        // The batch's end brings everything it staged.
        assert_eq!(
            names(&list.commit()),
            ["group 0", "changed Renamed", "added Two"]
        );
        assert_eq!(list.get(workspace).unwrap().group, None);
        assert!(list.groups().iter().any(|listed| listed.id == new_group));
    }

    #[test]
    fn a_workspace_waits_until_it_is_described_whole() {
        let mut list = WorkspaceList::default();
        let id = list.add_unready().unwrap();
        list.stage(id, Change::State(WorkspaceState::ACTIVE));
        assert!(list.commit().is_empty(), "not before it is ready");
        assert_eq!(list.snapshot(), []);
        list.stage(id, Change::Name("Named".into()));
        list.ready(id);
        assert_eq!(names(&list.commit()), ["added Named"]);
        let gone = list.add_unready().unwrap();
        list.remove(gone);
        assert!(list.commit().is_empty(), "never delivered, never removed");
    }

    #[test]
    fn a_group_that_is_not_listed_is_not_joined() {
        let mut list = WorkspaceList::default();
        let workspace = list.add().unwrap();
        list.stage(workspace, Change::Group(Some(GroupId(999))));
        list.commit();
        assert_eq!(list.get(workspace).unwrap().group, None);
    }

    #[test]
    fn what_the_compositor_sends_is_bounded() {
        let mut list = WorkspaceList::default();
        let id = list.add().unwrap();
        let long = "x".repeat(MAX_NAME_BYTES * 4);
        list.stage(id, Change::Name(format!("a\u{7}b\nc{long}")));
        list.stage(id, Change::Coordinates(vec![1; MAX_DIMENSIONS + 1]));
        list.commit();
        let workspace = list.get(id).unwrap();
        assert!(workspace.name.starts_with("ab c"));
        assert!(workspace.name.len() <= MAX_NAME_BYTES);
        assert_eq!(workspace.coordinates, Vec::<u32>::new());
        list.stage(id, Change::Coordinates(vec![2, 3]));
        list.commit();
        assert_eq!(list.get(id).unwrap().coordinates, [2, 3]);

        for _ in 1..MAX_WORKSPACES {
            assert!(list.add().is_some());
        }
        assert_eq!(list.add(), None, "no more than MAX_WORKSPACES");
        for _ in 0..MAX_GROUPS {
            assert!(list.add_group().is_some());
        }
        assert_eq!(list.add_group(), None, "no more than MAX_GROUPS");
    }

    #[test]
    fn order_moves_are_clamped() {
        let mut list = WorkspaceList::default();
        let a = list.add().unwrap();
        let b = list.add().unwrap();
        let c = list.add().unwrap();
        list.move_to(c, 0);
        assert_eq!(list.order(), [c, a, b]);
        list.move_to(c, 99);
        assert_eq!(list.order(), [a, b, c]);
    }
}

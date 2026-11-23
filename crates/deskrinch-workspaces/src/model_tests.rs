//! The workspace list's staging and delivery.

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

/// KWin removing its current desktop sends, as one batch, `activated` on
/// the next one, `deactivated` and `removed` on the current one, the
/// manager's `desktop_removed`, then one `done`: until the `done`, the
/// listing is the one before, never one with no current workspace.
#[test]
fn a_removal_is_listed_with_the_rest_of_its_batch() {
    let mut list = WorkspaceList::default();
    let next = list.add().unwrap();
    let current = list.add().unwrap();
    list.stage(next, Change::Name("Next".into()));
    list.stage(current, Change::Name("Current".into()));
    list.stage(current, Change::State(WorkspaceState::ACTIVE));
    list.commit();
    let before = list.snapshot();
    list.stage(next, Change::State(WorkspaceState::ACTIVE));
    list.stage(current, Change::State(WorkspaceState::empty()));
    list.remove(current);
    assert_eq!(
        list.snapshot(),
        before,
        "nothing of the batch before its end"
    );
    list.commit();
    let after = list.snapshot();
    assert_eq!(after.len(), 1, "{after:?}");
    assert_eq!(after[0].id, next);
    assert_eq!(after[0].state, WorkspaceState::ACTIVE);
}

/// A workspace moved is listed where it was until the batch's end, and
/// where it went from then on.
#[test]
fn a_move_is_listed_with_the_rest_of_its_batch() {
    let mut list = WorkspaceList::default();
    let first = list.add().unwrap();
    let second = list.add().unwrap();
    list.stage(first, Change::Name("First".into()));
    list.stage(second, Change::Name("Second".into()));
    list.commit();
    let before = list.snapshot();
    list.move_to(second, 0);
    list.stage(second, Change::Coordinates(vec![0]));
    list.stage(first, Change::Coordinates(vec![1]));
    assert_eq!(
        list.snapshot(),
        before,
        "nothing of the batch before its end"
    );
    list.commit();
    let order: Vec<_> = list.snapshot().iter().map(|w| w.id).collect();
    assert_eq!(order, [second, first]);
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

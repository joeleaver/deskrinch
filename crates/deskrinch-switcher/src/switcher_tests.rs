use super::*;

/// Workspace `id`, in group 0, activatable, not active.
fn entry(id: u32) -> Entry<u32, u32> {
    Entry {
        id,
        active: false,
        hidden: false,
        group: Some(0),
        coordinates: Vec::new(),
        activatable: true,
    }
}

/// A switcher with group 0 on `HEADLESS-1` and `entries`.
fn switcher(entries: Vec<Entry<u32, u32>>) -> Switcher<u32, u32> {
    let mut switcher = Switcher::new();
    switcher.apply(Change::Group {
        id: 0,
        outputs: vec!["HEADLESS-1".to_owned()],
    });
    for entry in entries {
        switcher.apply(Change::Workspace(entry));
    }
    switcher
}

/// The runs' buttons as (id, label) pairs.
fn shown(switcher: &Switcher<u32, u32>) -> Vec<Vec<(u32, String)>> {
    switcher
        .runs()
        .into_iter()
        .map(|run| run.buttons.into_iter().map(|b| (b.id, b.label)).collect())
        .collect()
}

fn pairs(ids: &[u32]) -> Vec<(u32, String)> {
    ids.iter()
        .enumerate()
        .map(|(index, &id)| (id, (index + 1).to_string()))
        .collect()
}

#[test]
fn a_button_shows_its_position_and_the_current_one_is_marked() {
    let mut first = entry(7);
    first.active = true;
    let switcher = switcher(vec![first, entry(3), entry(9)]);
    let runs = switcher.runs();
    let buttons = &runs[0].buttons;
    let labels: Vec<&str> = buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["1", "2", "3"]);
    assert!(buttons[0].current && !buttons[1].current && !buttons[2].current);
}

#[test]
fn without_coordinates_the_order_is_the_order_reported() {
    let switcher = switcher(vec![entry(5), entry(2), entry(8)]);
    assert_eq!(shown(&switcher), [pairs(&[5, 2, 8])]);
}

#[test]
fn with_coordinates_the_order_is_the_layout_row_by_row() {
    let place = |id, row, column| {
        let mut entry = entry(id);
        entry.coordinates = vec![row, column];
        entry
    };
    let switcher = switcher(vec![
        place(1, 1, 0),
        place(2, 0, 1),
        place(3, 0, 0),
        place(4, 1, 1),
    ]);
    assert_eq!(shown(&switcher), [pairs(&[3, 2, 1, 4])]);
}

#[test]
fn a_hidden_workspace_has_no_button_and_no_position() {
    let mut hidden = entry(2);
    hidden.hidden = true;
    let switcher = switcher(vec![entry(1), hidden, entry(3)]);
    assert_eq!(shown(&switcher), [pairs(&[1, 3])]);
    assert!(!switcher.activatable(2));
}

#[test]
fn a_change_keeps_its_place_and_a_removal_closes_the_gap() {
    let mut switcher = switcher(vec![entry(1), entry(2), entry(3)]);
    let mut changed = entry(2);
    changed.active = true;
    switcher.apply(Change::Workspace(changed));
    let runs = switcher.runs();
    assert_eq!(runs[0].buttons[1].label, "2");
    assert!(runs[0].buttons[1].current);
    switcher.apply(Change::Removed(1));
    assert_eq!(shown(&switcher), [pairs(&[2, 3])]);
    switcher.apply(Change::Workspace(entry(4)));
    assert_eq!(shown(&switcher), [pairs(&[2, 3, 4])], "added at the end");
}

#[test]
fn groups_are_runs_ordered_by_their_outputs_names_and_the_ungrouped_last() {
    let mut switcher = Switcher::new();
    switcher.apply(Change::Group {
        id: 10,
        outputs: vec!["HEADLESS-2".to_owned()],
    });
    switcher.apply(Change::Group {
        id: 20,
        outputs: vec!["HEADLESS-1".to_owned()],
    });
    let member = |id, group| {
        let mut entry = entry(id);
        entry.group = group;
        entry
    };
    for entry in [
        member(1, Some(10)),
        member(2, Some(20)),
        member(3, None),
        member(4, Some(10)),
        member(5, Some(99)),
    ] {
        switcher.apply(Change::Workspace(entry));
    }
    assert_eq!(
        shown(&switcher),
        [pairs(&[2]), pairs(&[1, 4]), pairs(&[3, 5])],
        "HEADLESS-1's group, HEADLESS-2's, then those of no known group"
    );
    switcher.apply(Change::GroupRemoved(20));
    assert_eq!(shown(&switcher), [pairs(&[1, 4]), pairs(&[2, 3, 5])]);
}

#[test]
fn a_click_asks_only_for_an_activatable_workspace() {
    let mut fixed = entry(2);
    fixed.activatable = false;
    let switcher = switcher(vec![entry(1), fixed]);
    assert!(switcher.activatable(1));
    assert!(!switcher.activatable(2));
    assert!(!switcher.activatable(3), "an unknown workspace");
}

#[test]
fn the_end_empties_it() {
    let mut switcher = switcher(vec![entry(1)]);
    switcher.apply(Change::Ended);
    assert_eq!(switcher.runs(), []);
    assert!(!switcher.activatable(1));
}

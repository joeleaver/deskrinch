use std::collections::BTreeSet;

use proptest::strategy::Strategy as _;

use super::*;
use crate::{Button, Change, Entry, Switcher};

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// The bars on the outputs named `names_`, one each.
fn bars(names_: &[&str]) -> Bars {
    Bars {
        named: names(names_),
        count: names_.len(),
    }
}

/// A run of one workspace, `id`, on the outputs named `outputs`.
fn run(id: u32, outputs: &[&str]) -> Run<u32> {
    Run {
        buttons: vec![Button {
            id,
            label: "1".to_owned(),
            current: true,
            activatable: true,
        }],
        outputs: names(outputs),
    }
}

/// The workspaces of the runs `bar` shows, in order.
fn shown(runs: &[Run<u32>], bar: Option<&str>, bars: &Bars) -> Vec<u32> {
    runs_on(runs, bar, bars)
        .iter()
        .flat_map(|run| run.buttons.iter().map(|button| button.id))
        .collect()
}

#[test]
fn each_bar_shows_its_own_groups_run() {
    let runs = [run(1, &["A"]), run(2, &["B"]), run(3, &["A", "B"])];
    let two = bars(&["A", "B"]);
    assert_eq!(shown(&runs, Some("A"), &two), [1, 3]);
    assert_eq!(shown(&runs, Some("B"), &two), [2, 3]);
}

#[test]
fn a_run_that_names_no_output_or_no_bars_is_on_every_bar() {
    // A group that names none, the workspaces in no group, and a group on
    // an output with no bar.
    let runs = [run(1, &["A"]), run(2, &[]), run(3, &["C"])];
    let two = bars(&["A", "B"]);
    assert_eq!(shown(&runs, Some("A"), &two), [1, 2, 3]);
    assert_eq!(shown(&runs, Some("B"), &two), [2, 3]);
}

#[test]
fn a_bar_with_no_name_and_a_lone_bar_show_every_run() {
    let runs = [run(1, &["A"]), run(2, &["B"]), run(3, &[])];
    let two = Bars {
        named: names(&["A"]),
        count: 2,
    };
    assert_eq!(shown(&runs, None, &two), [1, 2, 3]);
    assert_eq!(shown(&runs, Some(""), &two), [1, 2, 3]);
    assert_eq!(shown(&runs, Some("A"), &bars(&["A"])), [1, 2, 3]);
}

#[test]
fn two_groups_on_two_outputs_give_each_bar_its_own_run() {
    let mut switcher: Switcher<u32, u32> = Switcher::new();
    for (group, output) in [(0, "A"), (1, "B")] {
        switcher.apply(Change::Group {
            id: group,
            outputs: vec![output.to_owned()],
        });
    }
    for (id, group) in [(10, 0), (11, 0), (20, 1)] {
        switcher.apply(Change::Workspace(Entry {
            id,
            active: false,
            hidden: false,
            group: Some(group),
            coordinates: Vec::new(),
            activatable: true,
        }));
    }
    let runs = switcher.runs();
    let two = bars(&["A", "B"]);
    assert_eq!(shown(&runs, Some("A"), &two), [10, 11]);
    assert_eq!(shown(&runs, Some("B"), &two), [20]);
}

/// An output's name in the property: one of a few, or empty.
fn name() -> impl proptest::strategy::Strategy<Value = String> {
    proptest::sample::select(vec!["A", "B", "C", ""]).prop_map(str::to_owned)
}

proptest::proptest! {
    #![proptest_config(deskrinch_untrusted::fuzz::config())]

    /// With one bar every run is on it; with several, every run is on at
    /// least one bar, and a bar with no name shows every run; each bar's
    /// runs keep their order.
    #[test]
    fn every_run_is_on_at_least_one_bar(
        groups in proptest::collection::vec(proptest::collection::btree_set(name(), 0..3), 1..5),
        bar_outputs in proptest::collection::vec(proptest::option::of(name()), 1..4),
    ) {
        let runs: Vec<Run<u32>> = (0_u32..)
            .zip(&groups)
            .map(|(id, outputs)| Run {
                outputs: outputs.clone(),
                ..run(id, &[])
            })
            .collect();
        let bars = Bars {
            named: bar_outputs
                .iter()
                .flatten()
                .filter(|name| !name.is_empty())
                .cloned()
                .collect(),
            count: bar_outputs.len(),
        };
        let lists: Vec<Vec<u32>> = bar_outputs
            .iter()
            .map(|bar| shown(&runs, bar.as_deref(), &bars))
            .collect();
        let all: Vec<u32> = (0_u32..).take(runs.len()).collect();
        for id in &all {
            proptest::prop_assert!(lists.iter().any(|list| list.contains(id)), "run {id} on no bar");
        }
        for (bar, list) in bar_outputs.iter().zip(&lists) {
            proptest::prop_assert!(list.windows(2).all(|pair| pair[0] < pair[1]), "out of order");
            if bar_outputs.len() == 1 || bar.as_deref().is_none_or(str::is_empty) {
                proptest::prop_assert_eq!(list, &all);
            }
        }
    }
}

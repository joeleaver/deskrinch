use std::collections::BTreeSet;

use proptest::strategy::Strategy as _;

use super::*;
use crate::{Actions, Change, Input, TaskWindow, Taskbar, WindowState};

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// The bars, of those on the outputs named `bars`, that show a window that
/// knows `known`.
fn showing(known: &BTreeSet<String>, bars: &BTreeSet<String>) -> BTreeSet<String> {
    bars.iter()
        .filter(|bar| on_bar(known, Some(bar), bars))
        .cloned()
        .collect()
}

#[test]
fn a_window_is_on_the_bars_of_its_known_outputs() {
    let bars = names(&["A", "B", "C"]);
    assert_eq!(showing(&names(&["B"]), &bars), names(&["B"]));
    assert_eq!(showing(&names(&["A", "C"]), &bars), names(&["A", "C"]));
    // A known output with no bar is left out while another has one.
    assert_eq!(showing(&names(&["B", "D"]), &bars), names(&["B"]));
}

#[test]
fn a_window_whose_outputs_have_no_bar_is_on_every_bar() {
    let bars = names(&["A", "B"]);
    assert_eq!(showing(&names(&[]), &bars), bars);
    assert_eq!(showing(&names(&["C"]), &bars), bars);
}

#[test]
fn a_bar_on_an_output_with_no_name_shows_every_window() {
    let bars = names(&["A", "B"]);
    for known in [names(&["A"]), names(&[]), names(&["C"])] {
        assert!(on_bar(&known, None, &bars));
        assert!(on_bar(&known, Some(""), &bars));
    }
}

#[test]
fn a_report_of_none_keeps_what_was_known() {
    assert_eq!(known_outputs(&names(&["A"]), names(&[])), names(&["A"]));
    assert_eq!(known_outputs(&names(&["A"]), names(&["B"])), names(&["B"]));
    assert_eq!(known_outputs(&names(&[]), names(&[])), names(&[]));
}

#[test]
fn the_buttons_a_bar_shows_keep_their_order() {
    let button = |id: u32, outputs: &[&str]| Button {
        id,
        title: String::new(),
        app_id: String::new(),
        active: false,
        minimized: false,
        attention: false,
        outputs: names(outputs),
    };
    let buttons = [
        button(1, &["A"]),
        button(2, &["B"]),
        button(3, &[]),
        button(4, &["A", "B"]),
    ];
    let bars = names(&["A", "B"]);
    let ids = |bar| -> Vec<u32> {
        shown_on(&buttons, bar, &bars)
            .iter()
            .map(|b| b.id)
            .collect()
    };
    assert_eq!(ids(Some("A")), [1, 3, 4]);
    assert_eq!(ids(Some("B")), [2, 3, 4]);
    assert_eq!(ids(None), [1, 2, 3, 4]);
}

/// One step of a recorded sequence: the window reports outputs, or the
/// bars there are change (an output unplugged or plugged in).
#[derive(Debug, Clone, Copy)]
enum Step {
    Reports(&'static [&'static str]),
    Bars(&'static [&'static str]),
}

/// Replays `steps` on a model with one window that first reports `first`,
/// with bars on `bars`; after each step, checks the bars that show it
/// against `expected` and the rule: on the bars of its known outputs that
/// have one, else on every bar, and never on none.
fn replay(
    first: &'static [&'static str],
    bars: &'static [&'static str],
    steps: &[(Step, &[&str])],
) {
    let window = |reported: &[&str]| TaskWindow {
        id: 1_u32,
        title: "A".to_owned(),
        app_id: "org.example.A".to_owned(),
        state: WindowState::empty(),
        parent: None,
        outputs: names(reported),
    };
    let mut model = Taskbar::new();
    model.connected(Actions::all());
    model.input(Input::Change(Change::Added(window(first))));
    let mut bars = names(bars);
    for (n, (step, expected)) in steps.iter().enumerate() {
        match *step {
            Step::Reports(reported) => {
                model.input(Input::Change(Change::Changed(window(reported))));
            }
            Step::Bars(now) => bars = names(now),
        }
        let buttons = model.buttons();
        let known = &buttons[0].outputs;
        let on: BTreeSet<String> = bars
            .iter()
            .filter(|bar| shown_on(&buttons, Some(bar), &bars).len() == 1)
            .cloned()
            .collect();
        assert_eq!(on, names(expected), "step {n}, {step:?}");
        assert!(!on.is_empty(), "step {n}: on no bar");
        let with_a_bar: BTreeSet<String> = known.intersection(&bars).cloned().collect();
        if with_a_bar.is_empty() {
            assert_eq!(
                on, bars,
                "step {n}: its outputs have no bar, yet not every bar"
            );
        } else {
            assert_eq!(
                on, with_a_bar,
                "step {n}: not the bars of its known outputs"
            );
        }
    }
}

#[test]
fn kwins_unplug_and_replug_move_the_window_with_no_instant_on_every_bar() {
    replay(
        &["Virtual-0"],
        &["Virtual-0", "Virtual-1"],
        &[
            (Step::Reports(&["Virtual-0"]), &["Virtual-0"]),
            // The unplug: the window's only output gone, before KWin says
            // where it went; on every bar that remains.
            (Step::Bars(&["Virtual-1"]), &["Virtual-1"]),
            (Step::Reports(&["Virtual-0", "Virtual-1"]), &["Virtual-1"]),
            (Step::Reports(&["Virtual-1"]), &["Virtual-1"]),
            // The replug.
            (Step::Bars(&["Virtual-0", "Virtual-1"]), &["Virtual-1"]),
            (Step::Reports(&[]), &["Virtual-1"]),
            (Step::Reports(&["Virtual-1"]), &["Virtual-1"]),
            (Step::Reports(&[]), &["Virtual-1"]),
            (Step::Reports(&["Virtual-0"]), &["Virtual-0"]),
        ],
    );
}

#[test]
fn labwcs_unplug_and_replug_move_the_window_with_no_instant_on_every_bar() {
    replay(
        &["HEADLESS-1"],
        &["HEADLESS-1", "HEADLESS-2"],
        &[
            (Step::Bars(&["HEADLESS-2"]), &["HEADLESS-2"]),
            (
                Step::Reports(&["HEADLESS-1", "HEADLESS-2"]),
                &["HEADLESS-2"],
            ),
            (Step::Reports(&["HEADLESS-2"]), &["HEADLESS-2"]),
            (Step::Bars(&["HEADLESS-1", "HEADLESS-2"]), &["HEADLESS-2"]),
            (Step::Reports(&[]), &["HEADLESS-2"]),
            (Step::Reports(&["HEADLESS-1"]), &["HEADLESS-1"]),
        ],
    );
}

#[test]
fn sways_unplug_moves_the_window_and_its_silence_at_the_replug_leaves_it() {
    replay(
        &["HEADLESS-2"],
        &["HEADLESS-1", "HEADLESS-2"],
        &[
            (Step::Bars(&["HEADLESS-1"]), &["HEADLESS-1"]),
            (Step::Reports(&["HEADLESS-1"]), &["HEADLESS-1"]),
            (Step::Bars(&["HEADLESS-1", "HEADLESS-2"]), &["HEADLESS-1"]),
        ],
    );
}

#[test]
fn at_the_unplug_a_window_is_on_every_remaining_bar() {
    replay(
        &["A"],
        &["A", "B", "C"],
        &[(Step::Bars(&["B", "C"]), &["B", "C"])],
    );
}

/// An output's name in the properties: one of a few, or empty.
fn name() -> impl proptest::strategy::Strategy<Value = String> {
    proptest::sample::select(vec!["A", "B", "C", "D", ""]).prop_map(str::to_owned)
}

proptest::proptest! {
    #![proptest_config(deskrinch_untrusted::fuzz::config())]

    /// Every window is on at least one bar, whatever it knows and whatever
    /// the bars' outputs are called (one with no name, two alike, or names
    /// none of its outputs has); and while one of its known outputs has a
    /// named bar and no bar is unnamed, it is on exactly those bars.
    #[test]
    fn every_window_is_on_at_least_one_bar(
        known in proptest::collection::btree_set(name(), 0..4),
        outputs in proptest::collection::vec(proptest::option::of(name()), 1..5),
    ) {
        let bars: BTreeSet<String> = outputs
            .iter()
            .flatten()
            .filter(|name| !name.is_empty())
            .cloned()
            .collect();
        let shown: Vec<&Option<String>> = outputs
            .iter()
            .filter(|bar| on_bar(&known, bar.as_deref(), &bars))
            .collect();
        proptest::prop_assert!(!shown.is_empty(), "on none of {outputs:?}");
        let unnamed = outputs.iter().any(|bar| bar.as_deref().is_none_or(str::is_empty));
        if !unnamed && !known.is_disjoint(&bars) {
            for bar in &outputs {
                let named = bar.as_deref().unwrap_or_default();
                proptest::prop_assert_eq!(
                    on_bar(&known, Some(named), &bars),
                    known.contains(named),
                    "{} with {:?}", named, known
                );
            }
        }
    }

    /// A report of none keeps what was known, and any other report is what
    /// is known after it.
    #[test]
    fn the_known_outputs_are_the_last_reported_that_were_not_none(
        reports in proptest::collection::vec(proptest::collection::btree_set(name(), 0..3), 0..8),
    ) {
        let known = reports
            .iter()
            .fold(BTreeSet::new(), |known, reported| known_outputs(&known, reported.clone()));
        let last = reports.iter().rev().find(|reported| !reported.is_empty()).cloned();
        proptest::prop_assert_eq!(known, last.unwrap_or_default());
    }
}

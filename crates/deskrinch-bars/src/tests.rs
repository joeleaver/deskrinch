use proptest::strategy::Strategy as _;

use super::*;

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

/// The bars, of `bars`, that show something on `outputs`.
fn showing(outputs: &[&str], bars: &Bars) -> BTreeSet<String> {
    bars.named
        .iter()
        .filter(|bar| outputs_on_bar(&names(outputs), Some(bar), bars))
        .cloned()
        .collect()
}

#[test]
fn something_is_on_the_bars_of_its_outputs() {
    let bars = bars(&["A", "B", "C"]);
    assert_eq!(showing(&["B"], &bars), names(&["B"]));
    assert_eq!(showing(&["A", "C"], &bars), names(&["A", "C"]));
    // An output with no bar is left out while another has one.
    assert_eq!(showing(&["B", "D"], &bars), names(&["B"]));
}

#[test]
fn something_whose_outputs_have_no_bar_is_on_every_bar() {
    let bars = bars(&["A", "B"]);
    assert_eq!(showing(&["D"], &bars), bars.named);
    // Naming no output at all: every bar too.
    assert_eq!(showing(&[], &bars), bars.named);
}

#[test]
fn a_bar_on_an_output_with_no_name_shows_everything() {
    let bars = Bars {
        named: names(&["A"]),
        count: 2,
    };
    for outputs in [names(&["A"]), names(&["C"]), names(&[])] {
        assert!(outputs_on_bar(&outputs, None, &bars));
        assert!(outputs_on_bar(&outputs, Some(""), &bars));
    }
}

#[test]
fn with_one_bar_everything_is_on_it() {
    let one = bars(&["A"]);
    assert!(outputs_on_bar(&names(&["B"]), Some("A"), &one));
    assert!(outputs_on_bar(&names(&[]), Some("A"), &one));
}

/// An output's name in the properties: one of a few, or empty.
fn name() -> impl proptest::strategy::Strategy<Value = String> {
    proptest::sample::select(vec!["A", "B", "C", "D", ""]).prop_map(str::to_owned)
}

proptest::proptest! {
    #![proptest_config(deskrinch_untrusted::fuzz::config())]

    /// Whatever its outputs and whatever the bars' outputs are called (one
    /// with no name, two alike, or names none of its outputs has),
    /// something is on at least one bar; and while one of its outputs has a
    /// named bar and no bar is unnamed, with several bars, it is on exactly
    /// the bars of its outputs.
    #[test]
    fn everything_is_on_at_least_one_bar(
        outputs in proptest::collection::btree_set(name(), 0..4),
        bar_outputs in proptest::collection::vec(proptest::option::of(name()), 1..5),
    ) {
        let bars = Bars {
            named: bar_outputs
                .iter()
                .flatten()
                .filter(|name| !name.is_empty())
                .cloned()
                .collect(),
            count: bar_outputs.len(),
        };
        let shown = bar_outputs
            .iter()
            .filter(|bar| outputs_on_bar(&outputs, bar.as_deref(), &bars))
            .count();
        proptest::prop_assert!(shown > 0, "on none of {bar_outputs:?}");
        let unnamed = bar_outputs.iter().any(|bar| bar.as_deref().is_none_or(str::is_empty));
        if bar_outputs.len() > 1 && !unnamed && !outputs.is_disjoint(&bars.named) {
            for bar in &bar_outputs {
                let named = bar.as_deref().unwrap_or_default();
                proptest::prop_assert_eq!(
                    outputs_on_bar(&outputs, Some(named), &bars),
                    outputs.contains(named),
                    "{} with {:?}", named, outputs
                );
            }
        }
    }
}

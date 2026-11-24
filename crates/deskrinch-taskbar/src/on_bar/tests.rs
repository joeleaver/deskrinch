use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use proptest::strategy::Strategy as _;

use super::*;
use crate::{Actions, Change, Input, TaskWindow, Taskbar, WindowState};

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// The bars on the outputs named `names`, one each.
fn bars(names_: &[&str]) -> Bars {
    Bars {
        named: names(names_),
        count: names_.len(),
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// One window followed by a model, its changes heard at times from `t0`.
struct Replay {
    model: Taskbar<u32>,
    t0: Instant,
}

impl Replay {
    fn new() -> Self {
        let mut model = Taskbar::new();
        model.connected(Actions::all());
        Self {
            model,
            t0: Instant::now(),
        }
    }

    fn window(reported: &[&str]) -> TaskWindow<u32> {
        TaskWindow {
            id: 1,
            title: "A".to_owned(),
            app_id: "org.example.A".to_owned(),
            state: WindowState::empty(),
            parent: None,
            outputs: names(reported),
        }
    }

    /// The window announced at `at` ms, reporting `reported`.
    fn added(&mut self, reported: &[&str], at: u64) {
        let change = Change::Added(Self::window(reported));
        self.model.input(Input::Change(change, self.t0 + ms(at)));
    }

    /// The window reporting `reported` at `at` ms.
    fn reports(&mut self, reported: &[&str], at: u64) {
        let change = Change::Changed(Self::window(reported));
        self.model.input(Input::Change(change, self.t0 + ms(at)));
    }

    /// The bars, of `bars`, that show the window at `at` ms.
    fn on(&self, bars: &Bars, at: u64) -> BTreeSet<String> {
        let buttons = self.model.buttons();
        bars.named
            .iter()
            .filter(|bar| shown_on(&buttons, Some(bar), bars, self.t0 + ms(at)).len() == 1)
            .cloned()
            .collect()
    }
}

#[test]
fn a_placed_window_is_where_deskrinch_bars_puts_its_known_outputs() {
    let now = Instant::now();
    let three = bars(&["A", "B", "C"]);
    let unnamed = Bars {
        named: names(&["A"]),
        count: 2,
    };
    for (known, bar, bars) in [
        (names(&["B"]), Some("B"), &three),
        (names(&["B"]), Some("A"), &three),
        (names(&["D"]), Some("A"), &three),
        (names(&["C"]), None, &unnamed),
    ] {
        assert_eq!(
            on_bar(&Placement::Placed(known.clone()), bar, bars, now),
            outputs_on_bar(&known, bar, bars),
            "{known:?} on {bar:?}"
        );
    }
}

#[test]
fn an_unplaced_window_inside_its_grace_is_on_no_bar_not_even_an_unnamed_one() {
    let bars = Bars {
        named: names(&["A"]),
        count: 2,
    };
    let now = Instant::now();
    let unplaced = Placement::Unplaced { since: now };
    assert!(!on_bar(&unplaced, None, &bars, now));
    assert!(!on_bar(&unplaced, Some("A"), &bars, now));
}

#[test]
fn with_one_bar_an_unplaced_window_is_on_it_at_once() {
    let now = Instant::now();
    assert!(on_bar(
        &Placement::Unplaced { since: now },
        Some("A"),
        &bars(&["A"]),
        now
    ));
}

#[test]
fn an_unplaced_window_is_on_no_bar_for_the_grace_then_on_every_bar() {
    let bars = bars(&["A", "B"]);
    let since = Instant::now();
    let unplaced = Placement::Unplaced { since };
    let before = since + GRACE.saturating_sub(ms(1));
    assert!(!on_bar(&unplaced, Some("A"), &bars, before));
    assert!(!on_bar(&unplaced, Some("B"), &bars, before));
    assert!(on_bar(&unplaced, Some("A"), &bars, since + GRACE));
    assert!(on_bar(&unplaced, Some("B"), &bars, since + GRACE));
}

#[test]
fn a_report_of_none_keeps_the_placement() {
    let now = Instant::now();
    let placed = Placement::Placed(names(&["A"]));
    assert_eq!(Placement::after(Some(&placed), names(&[]), now), placed);
    assert_eq!(
        Placement::after(Some(&placed), names(&["B"]), now),
        Placement::Placed(names(&["B"]))
    );
    assert_eq!(
        Placement::after(None, names(&[]), now),
        Placement::Unplaced { since: now }
    );
    // An unplaced window keeps the time it was announced.
    let earlier = Placement::Unplaced { since: now };
    assert_eq!(
        Placement::after(Some(&earlier), names(&[]), now + ms(5)),
        earlier
    );
}

#[test]
fn the_buttons_a_bar_shows_keep_their_order() {
    let then = Instant::now();
    let now = then + GRACE;
    let button = |id: u32, placement: Placement| Button {
        id,
        title: String::new(),
        app_id: String::new(),
        active: false,
        minimized: false,
        attention: false,
        placement,
    };
    let placed = |outputs: &[&str]| Placement::Placed(names(outputs));
    let buttons = [
        button(1, placed(&["A"])),
        button(2, placed(&["B"])),
        button(3, Placement::Unplaced { since: then }),
        button(4, placed(&["A", "B"])),
        button(5, Placement::Unplaced { since: now }),
    ];
    let bars = bars(&["A", "B"]);
    let ids = |bar| -> Vec<u32> {
        shown_on(&buttons, bar, &bars, now)
            .iter()
            .map(|b| b.id)
            .collect()
    };
    assert_eq!(ids(Some("A")), [1, 3, 4]);
    assert_eq!(ids(Some("B")), [2, 3, 4]);
    assert_eq!(ids(None), [1, 2, 3, 4]);
}

fn unplaced(id: u32, since: Instant) -> Button<u32> {
    Button {
        id,
        title: String::new(),
        app_id: String::new(),
        active: false,
        minimized: false,
        attention: false,
        placement: Placement::Unplaced { since },
    }
}

/// Whether the unplaced window `button` is over its grace by `clock`, with
/// two bars.
fn over(button: &Button<u32>, clock: Instant) -> bool {
    on_bar(&button.placement, Some("A"), &bars(&["A", "B"]), clock)
}

mod recorded;
mod wake;

/// An output's name in the properties: one of a few, or empty.
fn name() -> impl proptest::strategy::Strategy<Value = String> {
    proptest::sample::select(vec!["A", "B", "C", "D", ""]).prop_map(str::to_owned)
}

proptest::proptest! {
    #![proptest_config(deskrinch_untrusted::fuzz::config())]

    /// With one bar every window is on it; with several, every window that
    /// is placed or whose grace is over is on at least one, whatever it
    /// knows and whatever the bars' outputs are called, and none inside its
    /// grace; and a placed window is where `deskrinch-bars` puts its known
    /// outputs, whose own tests have the rule.
    #[test]
    fn every_window_due_a_bar_is_on_at_least_one(
        known in proptest::option::of(proptest::collection::btree_set(name(), 1..4)),
        outputs in proptest::collection::vec(proptest::option::of(name()), 1..5),
        elapsed in 0_u64..600,
    ) {
        let since = Instant::now();
        let now = since + ms(elapsed);
        let placement = known
            .clone()
            .map_or(Placement::Unplaced { since }, Placement::Placed);
        let bars = Bars {
            named: outputs
                .iter()
                .flatten()
                .filter(|name| !name.is_empty())
                .cloned()
                .collect(),
            count: outputs.len(),
        };
        let shown = outputs
            .iter()
            .filter(|bar| on_bar(&placement, bar.as_deref(), &bars, now))
            .count();
        let due = outputs.len() == 1 || known.is_some() || now >= since + GRACE;
        if due {
            proptest::prop_assert!(shown > 0, "on none of {outputs:?}");
        } else {
            proptest::prop_assert_eq!(shown, 0, "inside its grace, on {:?}", outputs);
        }
        if let Some(known) = known {
            for bar in &outputs {
                proptest::prop_assert_eq!(
                    on_bar(&placement, bar.as_deref(), &bars, now),
                    outputs_on_bar(&known, bar.as_deref(), &bars),
                    "{:?} with {:?}", bar, known
                );
            }
        }
    }

    /// Whatever the announcements and however late each wake comes, every
    /// window whose grace has ended in real time is over by the clock once
    /// the wakes asked for have come, with nothing else to move it.
    #[test]
    fn every_grace_ended_in_real_time_is_over_after_the_wakes(
        announced in proptest::collection::vec(0_u64..1000, 1..6),
        seen_after in 0_u64..700,
        lateness in proptest::collection::vec(0_u64..700, 8),
    ) {
        let t0 = Instant::now();
        let windows: Vec<Button<u32>> = (0_u32..)
            .zip(&announced)
            .map(|(id, &at)| unplaced(id, t0 + ms(at)))
            .collect();
        // The UI sees them all once the last is announced, maybe later
        // (busy at a loaded start).
        let last = announced.iter().copied().max().unwrap_or_default();
        let mut now = t0 + ms(last + seen_after);
        let mut clock = t0;
        let mut wakes = 0;
        while let Some(wake) = next_wake(&windows, clock, now) {
            // A wake comes no earlier than asked, and maybe late.
            now = (now + wake.after).max(wake.at) + ms(lateness[wakes % lateness.len()]);
            clock = clock_on_wake(wake.at, now);
            wakes += 1;
            proptest::prop_assert!(wakes <= windows.len(), "more wakes than graces");
        }
        for window in &windows {
            let ended = window.placement.grace_ends().is_some_and(|end| end <= now);
            if ended {
                proptest::prop_assert!(over(window, clock), "{window:?} not over at {clock:?}");
            }
        }
    }

    /// A report of none keeps the placement, and any other report places
    /// the window on what it reported.
    #[test]
    fn the_known_outputs_are_the_last_reported_that_were_not_none(
        reports in proptest::collection::vec(proptest::collection::btree_set(name(), 0..3), 1..8),
    ) {
        let at = Instant::now();
        let placement = reports.iter().fold(None, |placement: Option<Placement>, reported| {
            Some(Placement::after(placement.as_ref(), reported.clone(), at))
        });
        let last = reports.iter().rev().find(|reported| !reported.is_empty()).cloned();
        proptest::prop_assert_eq!(
            placement,
            Some(last.map_or(Placement::Unplaced { since: at }, Placement::Placed))
        );
    }
}

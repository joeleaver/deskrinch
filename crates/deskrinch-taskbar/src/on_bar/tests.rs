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
fn a_placed_window_is_on_the_bars_of_its_known_outputs() {
    let bars = bars(&["A", "B", "C"]);
    let now = Instant::now();
    let showing = |known: &[&str]| -> BTreeSet<String> {
        let placement = Placement::Placed(names(known));
        bars.named
            .iter()
            .filter(|bar| on_bar(&placement, Some(bar), &bars, now))
            .cloned()
            .collect()
    };
    assert_eq!(showing(&["B"]), names(&["B"]));
    assert_eq!(showing(&["A", "C"]), names(&["A", "C"]));
    // A known output with no bar is left out while another has one.
    assert_eq!(showing(&["B", "D"]), names(&["B"]));
    // None of its known outputs has a bar: every bar.
    assert_eq!(showing(&["D"]), bars.named);
}

#[test]
fn a_bar_on_an_output_with_no_name_shows_every_placed_window() {
    let bars = Bars {
        named: names(&["A"]),
        count: 2,
    };
    let now = Instant::now();
    for known in [names(&["A"]), names(&["C"])] {
        assert!(on_bar(&Placement::Placed(known.clone()), None, &bars, now));
        assert!(on_bar(&Placement::Placed(known), Some(""), &bars, now));
    }
    // An unplaced window inside its grace is on no bar, unnamed ones too.
    assert!(!on_bar(
        &Placement::Unplaced { since: now },
        None,
        &bars,
        now
    ));
}

#[test]
fn with_one_bar_every_window_is_on_it() {
    let one = bars(&["A"]);
    let now = Instant::now();
    assert!(on_bar(
        &Placement::Unplaced { since: now },
        Some("A"),
        &one,
        now
    ));
    assert!(on_bar(
        &Placement::Placed(names(&["B"])),
        Some("A"),
        &one,
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

#[test]
fn two_ends_a_moment_apart_are_both_over_after_one_late_wake() {
    let t0 = Instant::now();
    let windows = [unplaced(1, t0), unplaced(2, t0 + ms(1))];
    let wake = next_wake(&windows, t0, t0).unwrap();
    assert_eq!(
        wake,
        Wake {
            at: t0 + GRACE,
            after: GRACE
        }
    );
    // The timer comes late, after both ends.
    let fired = t0 + GRACE + ms(5);
    let clock = clock_on_wake(wake.at, fired);
    assert!(windows.iter().all(|window| over(window, clock)));
    assert_eq!(next_wake(&windows, clock, fired), None);
}

#[test]
fn an_end_already_past_when_first_seen_is_woken_for_at_once() {
    let t0 = Instant::now();
    let windows = [unplaced(1, t0)];
    // The UI first sees it after its grace (busy at a loaded start).
    let seen = t0 + GRACE + ms(50);
    let wake = next_wake(&windows, t0, seen).unwrap();
    assert_eq!(
        wake,
        Wake {
            at: t0 + GRACE,
            after: Duration::ZERO
        }
    );
    let clock = clock_on_wake(wake.at, seen);
    assert!(over(&windows[0], clock));
}

#[test]
fn a_wake_that_comes_early_still_ends_the_grace_it_was_for() {
    let t0 = Instant::now();
    let windows = [unplaced(1, t0)];
    let wake = next_wake(&windows, t0, t0).unwrap();
    let clock = clock_on_wake(wake.at, t0 + GRACE.saturating_sub(ms(1)));
    assert!(over(&windows[0], clock));
}

#[test]
fn with_no_window_unplaced_nothing_wakes() {
    let t0 = Instant::now();
    let placed = Button {
        placement: Placement::Placed(names(&["A"])),
        ..unplaced(1, t0)
    };
    assert_eq!(next_wake(&[placed], t0, t0), None);
    assert_eq!(next_wake::<u32>(&[], t0, t0), None);
    // A grace already over by the clock needs no wake either.
    assert_eq!(next_wake(&[unplaced(2, t0)], t0 + GRACE, t0 + GRACE), None);
}

#[test]
fn a_window_closed_inside_its_grace_leaves_nothing_to_wake_for() {
    let mut replay = Replay::new();
    replay.added(&[], 0);
    let at = replay.t0 + ms(10);
    assert_eq!(
        next_wake(&replay.model.buttons(), replay.t0, at).map(|wake| wake.at),
        Some(replay.t0 + GRACE)
    );
    replay.model.input(Input::Change(Change::Removed(1), at));
    assert_eq!(next_wake(&replay.model.buttons(), replay.t0, at), None);
}

#[test]
fn sways_ordinary_window_placed_within_the_grace_is_never_on_another_bar() {
    // sway 1.11: its first batch carries no output; `output_enter` comes
    // 0.003 to 4.2 ms later.
    let bars = bars(&["HEADLESS-1", "HEADLESS-2"]);
    let mut replay = Replay::new();
    replay.added(&[], 0);
    assert_eq!(replay.on(&bars, 0), names(&[]));
    assert_eq!(replay.on(&bars, 4), names(&[]));
    replay.reports(&["HEADLESS-2"], 5);
    for at in [5, 299, 300, 2000] {
        assert_eq!(replay.on(&bars, at), names(&["HEADLESS-2"]), "at {at} ms");
    }
}

#[test]
fn a_window_sway_never_places_is_on_every_bar_after_the_grace() {
    // sway 1.11, a window opened on a workspace not shown: no output until
    // its workspace is.
    let bars = bars(&["HEADLESS-1", "HEADLESS-2"]);
    let mut replay = Replay::new();
    replay.added(&[], 0);
    assert_eq!(replay.on(&bars, 299), names(&[]));
    assert_eq!(replay.on(&bars, 300), bars.named);
    assert_eq!(replay.on(&bars, 4000), bars.named);
    // Its workspace shown: its own bar alone, in one step.
    replay.reports(&["HEADLESS-1"], 5000);
    assert_eq!(replay.on(&bars, 5000), names(&["HEADLESS-1"]));
}

#[test]
fn kwins_two_orders_put_the_window_on_its_own_bar_alone() {
    let bars = bars(&["Virtual-0", "Virtual-1"]);
    // Announced before it is mapped: the first geometry 24 ms later.
    let mut late = Replay::new();
    late.added(&[], 0);
    assert_eq!(late.on(&bars, 23), names(&[]));
    late.reports(&["Virtual-0"], 24);
    late.reports(&["Virtual-0"], 24);
    assert_eq!(late.on(&bars, 24), names(&["Virtual-0"]));
    assert_eq!(late.on(&bars, 1000), names(&["Virtual-0"]));
    // The geometry before `initial_state`: placed when announced.
    let mut early = Replay::new();
    early.added(&["Virtual-0"], 0);
    assert_eq!(early.on(&bars, 0), names(&["Virtual-0"]));
}

#[test]
fn labwcs_window_is_placed_when_announced() {
    let bars = bars(&["HEADLESS-1", "HEADLESS-2"]);
    let mut replay = Replay::new();
    replay.added(&["HEADLESS-2"], 0);
    assert_eq!(replay.on(&bars, 0), names(&["HEADLESS-2"]));
}

#[test]
fn a_bar_added_while_a_window_is_unplaced() {
    let one = bars(&["A"]);
    let two = bars(&["A", "B"]);
    let mut replay = Replay::new();
    replay.added(&[], 0);
    // One output: on its one bar at once.
    assert_eq!(replay.on(&one, 0), names(&["A"]));
    // A second output inside the grace: on none, being unplaced.
    assert_eq!(replay.on(&two, 100), names(&[]));
    // Placed: its own bar.
    replay.reports(&["B"], 150);
    assert_eq!(replay.on(&two, 150), names(&["B"]));
}

/// One step of an unplug and replug: the window reports outputs, or the
/// bars there are change.
#[derive(Debug, Clone, Copy)]
enum Step {
    Reports(&'static [&'static str]),
    Bars(&'static [&'static str]),
}

use Step::{Bars as Now, Reports};

/// Replays an unplug and replug: the window placed on `first`, then each
/// step, with the bars expected to show it after each.
fn replug(first: &[&str], start: &[&str], steps: &[(Step, &[&str])]) {
    let mut replay = Replay::new();
    replay.added(first, 0);
    let mut now = bars(start);
    for (n, (step, expected)) in steps.iter().enumerate() {
        match *step {
            Reports(reported) => replay.reports(reported, 1),
            Now(outputs) => now = bars(outputs),
        }
        assert_eq!(replay.on(&now, 1), names(expected), "step {n}, {step:?}");
    }
}

#[test]
fn kwins_unplug_and_replug_move_the_window_with_no_instant_on_every_bar() {
    // KWin 6.6.6: both, then the other; at the replug none, the other,
    // none, its own.
    replug(
        &["Virtual-0"],
        &["Virtual-0", "Virtual-1"],
        &[
            // The unplug: its only output gone, before KWin says where it
            // went; on every bar that remains.
            (Now(&["Virtual-1"]), &["Virtual-1"]),
            (Reports(&["Virtual-0", "Virtual-1"]), &["Virtual-1"]),
            (Reports(&["Virtual-1"]), &["Virtual-1"]),
            (Now(&["Virtual-0", "Virtual-1"]), &["Virtual-1"]),
            (Reports(&[]), &["Virtual-1"]),
            (Reports(&["Virtual-1"]), &["Virtual-1"]),
            (Reports(&[]), &["Virtual-1"]),
            (Reports(&["Virtual-0"]), &["Virtual-0"]),
        ],
    );
}

#[test]
fn labwcs_unplug_and_replug_move_the_window_with_no_instant_on_every_bar() {
    replug(
        &["HEADLESS-1"],
        &["HEADLESS-1", "HEADLESS-2"],
        &[
            (Now(&["HEADLESS-2"]), &["HEADLESS-2"]),
            (Reports(&["HEADLESS-1", "HEADLESS-2"]), &["HEADLESS-2"]),
            (Reports(&["HEADLESS-2"]), &["HEADLESS-2"]),
            (Now(&["HEADLESS-1", "HEADLESS-2"]), &["HEADLESS-2"]),
            (Reports(&[]), &["HEADLESS-2"]),
            (Reports(&["HEADLESS-1"]), &["HEADLESS-1"]),
        ],
    );
}

#[test]
fn sways_unplug_moves_the_window_and_its_silence_at_the_replug_leaves_it() {
    replug(
        &["HEADLESS-2"],
        &["HEADLESS-1", "HEADLESS-2"],
        &[
            (Now(&["HEADLESS-1"]), &["HEADLESS-1"]),
            (Reports(&["HEADLESS-1"]), &["HEADLESS-1"]),
            (Now(&["HEADLESS-1", "HEADLESS-2"]), &["HEADLESS-1"]),
        ],
    );
}

/// An output's name in the properties: one of a few, or empty.
fn name() -> impl proptest::strategy::Strategy<Value = String> {
    proptest::sample::select(vec!["A", "B", "C", "D", ""]).prop_map(str::to_owned)
}

proptest::proptest! {
    #![proptest_config(deskrinch_untrusted::fuzz::config())]

    /// With one bar every window is on it; with several, every window that
    /// is placed or whose grace is over is on at least one, whatever it
    /// knows and whatever the bars' outputs are called (one with no name,
    /// two alike, or names none of its outputs has); and a placed window
    /// with a known output on a named bar, where no bar is unnamed, is on
    /// exactly those bars.
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
        let unnamed = outputs.iter().any(|bar| bar.as_deref().is_none_or(str::is_empty));
        if let Some(known) = known
            && outputs.len() > 1
            && !unnamed
            && !known.is_disjoint(&bars.named)
        {
            for bar in &outputs {
                let named = bar.as_deref().unwrap_or_default();
                proptest::prop_assert_eq!(
                    on_bar(&placement, Some(named), &bars, now),
                    known.contains(named),
                    "{} with {:?}", named, known
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

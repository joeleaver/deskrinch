//! The cases recorded at each compositor's protocol (the module doc's
//! records), replayed through the model.

use super::*;

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

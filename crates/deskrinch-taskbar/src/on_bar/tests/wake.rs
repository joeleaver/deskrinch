//! The wakes for the graces: when the panel next looks, and what the
//! clock becomes when it does ([`next_wake`], [`clock_on_wake`]).

use super::*;

/// Two bars, where a new window waits its grace.
fn two() -> Bars {
    bars(&["A", "B"])
}

#[test]
fn two_ends_a_moment_apart_are_both_over_after_one_late_wake() {
    let t0 = Instant::now();
    let windows = [unplaced(1, t0), unplaced(2, t0 + ms(1))];
    let wake = next_wake(&windows, &two(), t0, t0).unwrap();
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
    assert_eq!(next_wake(&windows, &two(), clock, fired), None);
}

#[test]
fn an_end_already_past_when_first_seen_is_woken_for_at_once() {
    let t0 = Instant::now();
    let windows = [unplaced(1, t0)];
    // The UI first sees it after its grace (busy at a loaded start).
    let seen = t0 + GRACE + ms(50);
    let wake = next_wake(&windows, &two(), t0, seen).unwrap();
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
    let wake = next_wake(&windows, &two(), t0, t0).unwrap();
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
    assert_eq!(next_wake(&[placed], &two(), t0, t0), None);
    assert_eq!(next_wake::<u32>(&[], &two(), t0, t0), None);
    // A grace already over by the clock needs no wake either.
    assert_eq!(
        next_wake(&[unplaced(2, t0)], &two(), t0 + GRACE, t0 + GRACE),
        None
    );
}

#[test]
fn a_window_closed_inside_its_grace_leaves_nothing_to_wake_for() {
    let mut replay = Replay::new();
    replay.added(&[], 0);
    let at = replay.t0 + ms(10);
    assert_eq!(
        next_wake(&replay.model.buttons(), &two(), replay.t0, at).map(|wake| wake.at),
        Some(replay.t0 + GRACE)
    );
    replay.model.input(Input::Change(Change::Removed(1), at));
    assert_eq!(
        next_wake(&replay.model.buttons(), &two(), replay.t0, at),
        None
    );
}

#[test]
fn with_one_bar_or_none_nothing_wakes_for_a_window_never_placed() {
    let t0 = Instant::now();
    let windows = [unplaced(1, t0)];
    assert_eq!(next_wake(&windows, &bars(&["A"]), t0, t0), None);
    let unnamed = Bars {
        named: BTreeSet::new(),
        count: 1,
    };
    assert_eq!(next_wake(&windows, &unnamed, t0, t0), None);
    assert_eq!(next_wake(&windows, &bars(&[]), t0, t0), None);
    // A second bar plugged in: the grace counts from the announcement.
    assert_eq!(
        next_wake(&windows, &two(), t0, t0 + ms(100)),
        Some(Wake {
            at: t0 + GRACE,
            after: GRACE.saturating_sub(ms(100))
        })
    );
}

#[test]
fn the_time_matters_only_with_several_bars_and_a_window_unplaced() {
    let t0 = Instant::now();
    let placed = Button {
        placement: Placement::Placed(names(&["A"])),
        ..unplaced(1, t0)
    };
    let waiting = unplaced(2, t0);
    assert!(hangs_on_time(&[placed.clone(), waiting.clone()], &two()));
    assert!(!hangs_on_time(std::slice::from_ref(&placed), &two()));
    assert!(!hangs_on_time::<u32>(&[], &two()));
    assert!(!hangs_on_time(&[placed, waiting], &bars(&["A"])));
}

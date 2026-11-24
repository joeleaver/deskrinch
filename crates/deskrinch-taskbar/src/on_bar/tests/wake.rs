//! The wakes for the graces: when the panel next looks, and what the
//! clock becomes when it does ([`next_wake`], [`clock_on_wake`]).

use super::*;

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

use super::*;

fn id(n: u32) -> NotificationId {
    NotificationId::new(n).unwrap()
}

fn ids(ns: &[u32]) -> Vec<NotificationId> {
    ns.iter().map(|n| id(*n)).collect()
}

/// The container: 356 wide at x 12, from y 12, `height` high.
fn room(height: f32) -> Place {
    Place {
        x: 12.0,
        y: 12.0,
        width: 356.0,
        height,
    }
}

fn at(x: f32, y: f32, height: f32) -> Place {
    Place {
        x,
        y,
        width: 356.0,
        height,
    }
}

#[test]
fn critical_ones_first_then_the_others_newest_first_within_each() {
    // In the model's order, first shown first: 1, 2 (critical), 3, 4
    // (critical), 5.
    let live = [
        (id(1), false),
        (id(2), true),
        (id(3), false),
        (id(4), true),
        (id(5), false),
    ];
    assert_eq!(order(&live), ids(&[4, 2, 5, 3, 1]));
    assert_eq!(order(&[]), ids(&[]));
}

#[test]
fn every_popup_in_the_first_column_and_whole_is_shown() {
    let places = BTreeMap::from([
        (id(3), at(12.0, 12.0, 100.0)),
        (id(2), at(12.0, 124.0, 150.0)),
        (id(1), at(12.0, 286.0, 120.0)),
    ]);
    let fit = shown(&ids(&[3, 2, 1]), &places, room(696.0), None);
    assert_eq!(fit.shown, ids(&[3, 2, 1]));
    assert_eq!(fit.waiting, ids(&[]));
}

#[test]
fn a_popup_wrapped_into_the_second_column_waits() {
    let places = BTreeMap::from([
        (id(3), at(12.0, 12.0, 300.0)),
        (id(2), at(12.0, 324.0, 300.0)),
        // Wrapped: past the first column, at the top of the second.
        (id(1), at(380.0, 12.0, 120.0)),
    ]);
    let fit = shown(&ids(&[3, 2, 1]), &places, room(696.0), None);
    assert_eq!(fit.shown, ids(&[3, 2]));
    assert_eq!(fit.waiting, ids(&[1]));
}

#[test]
fn a_first_popup_taller_than_the_room_waits_and_is_not_drawn_cut() {
    let places = BTreeMap::from([
        (id(2), at(12.0, 12.0, 400.0)),
        (id(1), at(380.0, 12.0, 100.0)),
    ]);
    let fit = shown(&ids(&[2, 1]), &places, room(300.0), None);
    assert_eq!(fit.shown, ids(&[]));
    assert_eq!(fit.waiting, ids(&[2, 1]));
}

#[test]
fn a_popup_not_laid_out_yet_waits() {
    let places = BTreeMap::from([(id(2), at(12.0, 12.0, 100.0)), (id(1), at(0.0, 0.0, 0.0))]);
    let fit = shown(&ids(&[3, 2, 1]), &places, room(696.0), None);
    assert_eq!(fit.shown, ids(&[2]));
    assert_eq!(fit.waiting, ids(&[3, 1]));
}

#[test]
fn a_bottom_a_rounding_past_the_room_is_still_whole() {
    let places = BTreeMap::from([(id(1), at(12.0, 12.0, 696.6))]);
    assert_eq!(
        shown(&ids(&[1]), &places, room(696.0), None).shown,
        ids(&[1])
    );
    let places = BTreeMap::from([(id(1), at(12.0, 12.0, 699.0))]);
    assert_eq!(
        shown(&ids(&[1]), &places, room(696.0), None).waiting,
        ids(&[1])
    );
}

// ── The count line's own room ─────────────────────────────────────────

/// The column around a 600-high container with the count line under it:
/// padded 12, the container from 12 to 612, the gap, 12, and the line, 20
/// high, to 644, then the padding to 656. With no line the container would
/// be 632 high.
fn column() -> Place {
    Place {
        x: 0.0,
        y: 0.0,
        width: 380.0,
        height: 656.0,
    }
}

/// Three popups `height` high: the first two in the first column, the
/// third wrapped into the second.
fn three(height: f32) -> BTreeMap<NotificationId, Place> {
    BTreeMap::from([
        (id(3), at(12.0, 12.0, height)),
        (id(2), at(12.0, 24.0 + height, height)),
        (id(1), at(380.0, 12.0, height)),
    ])
}

#[test]
fn all_shown_once_the_line_s_room_is_given_back() {
    // 3 × 200 and two gaps: 624, more than the room, within the room and
    // the line's (632).
    let fit = shown(&ids(&[3, 2, 1]), &three(200.0), room(600.0), Some(column()));
    assert_eq!(fit.shown, ids(&[3, 2, 1]));
    assert_eq!(fit.waiting, ids(&[]));
}

#[test]
fn not_all_fit_in_the_room_and_the_line_s_and_the_layout_s_fit_stands() {
    // 3 × 205 and two gaps: 639.
    let fit = shown(&ids(&[3, 2, 1]), &three(205.0), room(600.0), Some(column()));
    assert_eq!(fit.shown, ids(&[3, 2]));
    assert_eq!(fit.waiting, ids(&[1]));
}

#[test]
fn several_waiting_and_room_for_one_more_all_still_wait() {
    let places = BTreeMap::from([
        (id(4), at(12.0, 12.0, 200.0)),
        (id(3), at(12.0, 224.0, 200.0)),
        (id(2), at(380.0, 12.0, 200.0)),
        (id(1), at(380.0, 224.0, 200.0)),
    ]);
    let fit = shown(&ids(&[4, 3, 2, 1]), &places, room(600.0), Some(column()));
    assert_eq!(fit.shown, ids(&[4, 3]));
    assert_eq!(fit.waiting, ids(&[2, 1]));
}

#[test]
fn nothing_is_assumed_of_a_column_or_a_popup_not_laid_out_yet() {
    let fit = shown(&ids(&[3, 2, 1]), &three(200.0), room(600.0), None);
    assert_eq!(fit.waiting, ids(&[1]));
    let unplaced = Some(at(0.0, 0.0, 0.0));
    let fit = shown(&ids(&[3, 2, 1]), &three(200.0), room(600.0), unplaced);
    assert_eq!(fit.waiting, ids(&[1]));
    let mut places = three(200.0);
    places.remove(&id(1));
    let fit = shown(&ids(&[3, 2, 1]), &places, room(600.0), Some(column()));
    assert_eq!(fit.waiting, ids(&[1]));
}

#[test]
fn with_no_line_the_judgment_never_goes_against_the_layout() {
    // The column of a 600-high container and no line: 624 does not fit.
    let column = Some(Place {
        x: 0.0,
        y: 0.0,
        width: 380.0,
        height: 624.0,
    });
    let fit = shown(&ids(&[3, 2, 1]), &three(200.0), room(600.0), column);
    assert_eq!(fit.waiting, ids(&[1]));
}

#[test]
fn a_sum_within_a_pixel_of_the_room_is_not_taken_to_fit() {
    // 630 fits the 632 with a pixel to spare; 631.5 does not.
    let fit = shown(&ids(&[3, 2, 1]), &three(202.0), room(600.0), Some(column()));
    assert_eq!(fit.waiting, ids(&[]));
    let fit = shown(&ids(&[3, 2, 1]), &three(202.5), room(600.0), Some(column()));
    assert_eq!(fit.waiting, ids(&[1]));
}

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
    let fit = shown(&ids(&[3, 2, 1]), &places, room(696.0));
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
    let fit = shown(&ids(&[3, 2, 1]), &places, room(696.0));
    assert_eq!(fit.shown, ids(&[3, 2]));
    assert_eq!(fit.waiting, ids(&[1]));
}

#[test]
fn a_first_popup_taller_than_the_room_waits_and_is_not_drawn_cut() {
    let places = BTreeMap::from([
        (id(2), at(12.0, 12.0, 400.0)),
        (id(1), at(380.0, 12.0, 100.0)),
    ]);
    let fit = shown(&ids(&[2, 1]), &places, room(300.0));
    assert_eq!(fit.shown, ids(&[]));
    assert_eq!(fit.waiting, ids(&[2, 1]));
}

#[test]
fn a_popup_not_laid_out_yet_waits() {
    let places = BTreeMap::from([(id(2), at(12.0, 12.0, 100.0)), (id(1), at(0.0, 0.0, 0.0))]);
    let fit = shown(&ids(&[3, 2, 1]), &places, room(696.0));
    assert_eq!(fit.shown, ids(&[2]));
    assert_eq!(fit.waiting, ids(&[3, 1]));
}

#[test]
fn a_bottom_a_rounding_past_the_room_is_still_whole() {
    let places = BTreeMap::from([(id(1), at(12.0, 12.0, 696.6))]);
    assert_eq!(shown(&ids(&[1]), &places, room(696.0)).shown, ids(&[1]));
    let places = BTreeMap::from([(id(1), at(12.0, 12.0, 699.0))]);
    assert_eq!(shown(&ids(&[1]), &places, room(696.0)).waiting, ids(&[1]));
}

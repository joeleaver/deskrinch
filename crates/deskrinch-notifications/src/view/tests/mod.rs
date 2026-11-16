//! The fit through rinch's test harness: the column's content mounted at
//! several window heights, with no compositor ([`harness`]). The cases the
//! compositor tests check on real screens, here in the ordinary test run:
//! everything fits; one more than fits; a critical one before newer ones;
//! the count line's room moving the cut; a first popup taller than the
//! room; the height changed under a full column. And the count line never
//! holding its own room ([`fit::shown`](crate::fit::shown)): a waiting
//! popup that fits once the line goes is shown, after a resize or a close,
//! and around that edge the fit settles at once and holds.
//!
//! The popups' heights are the host's fonts', so the tests hold to how
//! popups stand to each other and to the room, never to a pixel count.

mod harness;

use harness::Harness;

/// Popups of one line each, all the same height.
fn summary(n: usize) -> String {
    format!("Popup {n:02}")
}

/// Sends popups until one waits; their ids, oldest first.
fn fill(harness: &mut Harness) -> Vec<deskrinch_notification_service::NotificationId> {
    let mut ids = Vec::new();
    for n in 1..=40 {
        ids.push(harness.send(&summary(n), "", false));
        if !harness.fit().1.is_empty() {
            return ids;
        }
    }
    panic!("forty popups all fit: {:?}", harness.popups());
}

/// A height at which the `n` popups now live (no count line) fit with a
/// pixel to spare and no more: the room's bottom just under the last one's.
fn exact_height(harness: &Harness, n: usize) -> u32 {
    let popups = harness.popups();
    assert_eq!(popups.len(), n);
    harness.height_for(popups.last().unwrap().bottom, 1)
}

#[test]
fn everything_fits() {
    let mut harness = Harness::at(600);
    let ids: Vec<_> = (1..=3)
        .map(|n| harness.send(&summary(n), "", false))
        .collect();
    // The newest first.
    assert_eq!(
        harness.fit(),
        (vec![summary(3), summary(2), summary(1)], vec![])
    );
    harness.assert_consistent();
    assert!(ids.iter().all(|id| !harness.paused(*id)));
}

#[test]
fn one_more_than_fits_waits_the_oldest_held() {
    let mut harness = Harness::at(300);
    let ids = fill(&mut harness);
    harness.assert_consistent();
    let (shown, waiting) = harness.fit();
    // The waiting ones are the oldest, after every shown one.
    let order: Vec<String> = (1..=ids.len()).rev().map(summary).collect();
    assert_eq!([shown.clone(), waiting.clone()].concat(), order);
    let (held, running) = ids.split_at(waiting.len());
    assert!(
        held.iter().all(|id| harness.paused(*id)),
        "a waiting one runs"
    );
    assert!(
        running.iter().all(|id| !harness.paused(*id)),
        "a shown one is held"
    );
}

#[test]
fn a_critical_one_goes_before_newer_ones() {
    let mut harness = Harness::at(300);
    let ids = fill(&mut harness);
    let waiting_before = harness.fit().1.len();
    harness.send("Critical", "", true);
    harness.assert_consistent();
    let popups = harness.popups();
    let first = &popups[0];
    assert_eq!(first.summary, "Critical");
    assert!(first.critical && !first.waiting, "{first:?}");
    let (_, waiting) = harness.fit();
    assert!(
        waiting.len() > waiting_before,
        "nothing made room: {popups:?}"
    );
    // The oldest waits.
    assert_eq!(waiting.last(), Some(&summary(1)));
    assert!(harness.paused(ids[0]));
}

/// A full column with less than the count line's room left under it (a
/// pixel): one more popup brings the line, which takes the room of the
/// last shown too, so exactly two wait and the line says so. The close of
/// the newest brings both back, and the line goes. (The compositor's
/// frames test counts that close's frames: one.)
#[test]
fn the_count_line_s_room_moves_the_cut() {
    let mut harness = Harness::at(800);
    let mut ids: Vec<_> = (1..=3)
        .map(|n| harness.send(&summary(n), "", false))
        .collect();
    let height = exact_height(&harness, 3);
    harness.resize(height);
    assert_eq!(
        harness.fit().1,
        Vec::<String>::new(),
        "three do not fit at {height}"
    );
    // One more: four of the same height cannot fit where three just did,
    // and the count line takes its room under the container, so the third
    // no longer fits either.
    ids.push(harness.send(&summary(4), "", false));
    harness.assert_consistent();
    assert_eq!(
        harness.fit(),
        (vec![summary(4), summary(3)], vec![summary(2), summary(1)])
    );
    assert_eq!(harness.count_line().as_deref(), Some("2 more"));
    assert!(harness.paused(ids[0]) && harness.paused(ids[1]));

    harness.close(ids[3]);
    harness.assert_consistent();
    assert_eq!(
        harness.fit(),
        (vec![summary(3), summary(2), summary(1)], vec![])
    );
    assert_eq!(harness.count_line(), None);
    assert!(ids[..3].iter().all(|id| !harness.paused(*id)));
}

#[test]
fn a_first_popup_taller_than_the_room_waits_hidden() {
    let mut harness = Harness::at(120);
    let six = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6";
    let id = harness.send("Tall", six, false);
    harness.assert_consistent();
    assert_eq!(harness.fit(), (vec![], vec!["Tall".to_owned()]));
    assert!(harness.paused(id));
}

#[test]
fn the_height_changed_under_a_full_column_holds_one_back_and_brings_it_back() {
    let mut harness = Harness::at(800);
    let ids: Vec<_> = (1..=3)
        .map(|n| harness.send(&summary(n), "", false))
        .collect();
    let height = exact_height(&harness, 3);
    harness.resize(height);
    assert_eq!(harness.fit().1, Vec::<String>::new());

    harness.resize(height - 20);
    harness.assert_consistent();
    let (shown, waiting) = harness.fit();
    assert!(!waiting.is_empty(), "the room shrank and all still fit");
    assert_eq!(waiting.last(), Some(&summary(1)), "the oldest waits first");
    assert!(harness.paused(ids[0]));
    assert!(shown.contains(&summary(3)));

    harness.resize(height);
    harness.assert_consistent();
    assert_eq!(
        harness.fit(),
        (vec![summary(3), summary(2), summary(1)], vec![])
    );
    assert!(
        ids.iter().all(|id| !harness.paused(*id)),
        "one is still held"
    );
}

/// A close that leaves room for the waiting one only once the count line
/// goes: it is shown, and the line goes. The oldest is a line taller than
/// the others, less than the line's room taller.
#[test]
fn a_close_that_frees_room_only_without_the_line_brings_the_waiting_one_back() {
    let mut harness = Harness::at(800);
    let tall = harness.send("Tall", "one more line", false);
    let short: Vec<_> = (1..=3)
        .map(|n| harness.send(&summary(n), "", false))
        .collect();
    // Shown newest first: three short ones, then the tall one.
    let popups = harness.popups();
    let gap = popups[1].top - popups[0].bottom;
    let (short_height, tall_height) = (
        popups[0].bottom - popups[0].top,
        popups[3].bottom - popups[3].top,
    );
    // The room, with no line, of two short ones and the tall one, and two
    // pixels.
    let after = tall_height + 2.0 * (short_height + gap);
    let height = harness.height_for(harness.room_top() + after, 2);
    harness.resize(height);
    harness.assert_consistent();
    let (_, waiting) = harness.fit();
    assert_eq!(
        waiting.last(),
        Some(&"Tall".to_owned()),
        "{:?}",
        harness.popups()
    );
    assert!(harness.paused(tall));

    harness.close(short[2]);
    harness.assert_consistent();
    assert_eq!(
        harness.fit(),
        (vec![summary(2), summary(1), "Tall".to_owned()], vec![])
    );
    assert!(!harness.paused(tall));
}

/// Around the edge where three popups just fit, each way, a pixel at a
/// time: the fit settles at once and holds (a second settle changes
/// nothing), all three are shown wherever they fit with a pixel to spare,
/// the oldest waits wherever they do not fit, and the pixel between holds
/// whichever state it came in with.
#[test]
fn around_the_edge_the_fit_settles_at_once_and_holds_each_way() {
    let mut harness = Harness::at(800);
    for n in 1..=3 {
        harness.send(&summary(n), "", false);
    }
    let edge = exact_height(&harness, 3);
    let all = (vec![summary(3), summary(2), summary(1)], vec![]);
    let one_waits = (vec![summary(3), summary(2)], vec![summary(1)]);
    let down = (edge - 6..=edge + 4).rev();
    let up = edge - 6..=edge + 4;
    for height in down.chain(up) {
        harness.resize(height);
        harness.assert_consistent();
        let fit = harness.fit();
        harness.app.settle();
        assert_eq!(harness.fit(), fit, "the fit moved on its own at {height}");
        if height >= edge {
            assert_eq!(fit, all, "at {height}, the edge {edge}");
        } else if height + 1 < edge {
            assert_eq!(fit, one_waits, "at {height}, the edge {edge}");
        } else {
            assert!(fit == all || fit == one_waits, "at {height}: {fit:?}");
        }
    }
}

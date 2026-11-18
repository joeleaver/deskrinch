use super::*;

/// A run of `count` buttons, ids from 0, `current` marked.
fn run(count: u32, current: Option<u32>) -> Run<u32> {
    Run {
        buttons: (0..count)
            .map(|id| Button {
                id,
                label: (id + 1).to_string(),
                current: Some(id) == current,
                activatable: true,
            })
            .collect(),
        outputs: std::collections::BTreeSet::new(),
    }
}

/// 28 pixels a button, 4 between, 16 between runs, 24 for a count.
fn room(width: f32) -> Room {
    Room {
        width,
        button: 28.0,
        gap: 4.0,
        run_gap: 16.0,
        count_width: 24.0,
    }
}

fn ids(fitted: &Fitted<u32>) -> Vec<u32> {
    fitted.shown.iter().map(|button| button.id).collect()
}

#[test]
fn buttons_that_fit_are_all_shown() {
    // Four take 4 * 28 + 3 * 4 = 124.
    let fitted = fit(&[run(4, Some(1))], room(124.0));
    assert_eq!(ids(&fitted[0]), [0, 1, 2, 3]);
    assert_eq!((fitted[0].before, fitted[0].after), (0, 0));
}

#[test]
fn past_the_room_those_furthest_from_the_current_are_counted_at_either_end() {
    // Ten in 200, the sixth current: k buttons and two counts take
    // 32k - 4 + 56, so k = 4, centred on the sixth.
    let fitted = fit(&[run(10, Some(5))], room(200.0));
    assert_eq!(ids(&fitted[0]), [4, 5, 6, 7]);
    assert_eq!((fitted[0].before, fitted[0].after), (4, 2));
}

#[test]
fn at_an_end_only_one_count_is_drawn_and_its_room_goes_to_buttons() {
    // The last current: one count, so 32k - 4 + 28 <= 200 gives k = 5.
    let fitted = fit(&[run(10, Some(9))], room(200.0));
    assert_eq!(ids(&fitted[0]), [5, 6, 7, 8, 9]);
    assert_eq!((fitted[0].before, fitted[0].after), (5, 0));
    let first = fit(&[run(10, Some(0))], room(200.0));
    assert_eq!(ids(&first[0]), [0, 1, 2, 3, 4]);
    assert_eq!((first[0].before, first[0].after), (0, 5));
}

#[test]
fn the_current_one_is_shown_even_with_no_room() {
    let fitted = fit(&[run(5, Some(3))], room(10.0));
    assert_eq!(ids(&fitted[0]), [3]);
    assert_eq!((fitted[0].before, fitted[0].after), (3, 1));
    let none_current = fit(&[run(5, None)], room(10.0));
    assert_eq!(
        ids(&none_current[0]),
        [0],
        "the first, where none is current"
    );
}

#[test]
fn runs_share_the_room_equally() {
    // Two runs in 300: (300 - 16) / 2 = 142 each; four buttons take 124.
    let fitted = fit(&[run(4, Some(0)), run(6, Some(5))], room(300.0));
    assert_eq!(ids(&fitted[0]), [0, 1, 2, 3]);
    // Six take 188 > 142: one count, 32k - 4 + 28 <= 142 gives k = 3.
    assert_eq!(ids(&fitted[1]), [3, 4, 5]);
    assert_eq!((fitted[1].before, fitted[1].after), (3, 0));
}

#[test]
fn nothing_to_show_fits() {
    assert_eq!(fit::<u32>(&[], room(100.0)), []);
    let fitted = fit(&[run(0, None)], room(100.0));
    assert_eq!(fitted[0].shown, []);
}

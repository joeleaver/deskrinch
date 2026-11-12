//! Clicks on the launcher's results, through `deskrinch-test-input`'s
//! pointer, on [`crate::screen`]'s fixture: a press and a release of the
//! primary button on one result start it, on two different ones nothing.
//!
//! The results are found on the screen, not where a theme puts them: the
//! runs of pixel rows, below the query field's, in which something across
//! the launcher is not its background (the selected result's band, another
//! result's name). The suite runs at scale 1 only, where the image's pixels
//! are the output's logical pixels, which is what the pointer is moved in.

use deskrinch_test_input::Button;
use deskrinch_test_screen::{Image, Rgba};

use crate::screen::{COLUMN, HEIGHT, Shown, WIDTH, args, at_centre, the_one_started, token};
use crate::support::stays;

/// The query both tests' results come from: "Plain Recorder" first (its
/// name starts with it) and selected, then "Recorder Plain".
const QUERY: &str = "plain";

/// The middle of each result drawn, top to bottom, as a point on the
/// output: the launcher's middle across, each run's middle down.
fn results(image: &Image) -> Vec<(f64, f64)> {
    let Some((left, top)) = at_centre(image) else {
        return Vec::new();
    };
    let background: Option<Rgba> = image.pixel(left + 2, top + 2);
    let busy = |row: u32| {
        (left + COLUMN..left + WIDTH - COLUMN).any(|x| image.pixel(x, row) != background)
    };
    let mut runs: Vec<(u32, u32)> = Vec::new();
    let mut start = None;
    for row in top..top + HEIGHT {
        match (busy(row), start) {
            (true, None) => start = Some(row),
            (false, Some(first)) => {
                runs.push((first, row));
                start = None;
            }
            _ => {}
        }
    }
    // The first run is the query field.
    runs.iter()
        .skip(1)
        .map(|&(first, end)| {
            (
                f64::from(left + WIDTH / 2),
                f64::from(first + (end - first) / 2),
            )
        })
        .collect()
}

/// A launcher showing [`QUERY`]'s two results; their points.
fn two_results() -> (Shown, [(f64, f64); 2]) {
    let mut shown = Shown::start("");
    shown.input.type_text(QUERY).unwrap();
    let image = shown.see("two results", |image| results(image).len() == 2);
    let found = results(&image);
    (shown, [found[0], found[1]])
}

fn click(shown: &mut Shown, (x, y): (f64, f64)) {
    let output = shown.output.clone();
    shown.input.move_pointer(&output, x, y).unwrap();
    shown.input.click(Button::PRIMARY).unwrap();
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_click_on_the_selected_result_starts_it_with_a_token_and_hides() {
    let (mut shown, [first, _]) = two_results();
    click(&mut shown, first);
    let record = the_one_started(&shown);
    assert!(
        !token(&record).is_empty(),
        "no activation token: {record:?}"
    );
    assert_eq!(args(&record), ["Plain"]);
    shown.see("the launcher hidden", |image| at_centre(image).is_none());
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_click_on_the_other_result_starts_that_one() {
    let (mut shown, [_, second]) = two_results();
    click(&mut shown, second);
    let record = the_one_started(&shown);
    assert!(
        !token(&record).is_empty(),
        "no activation token: {record:?}"
    );
    assert_eq!(args(&record), ["Other"], "not the selected one");
    shown.see("the launcher hidden", |image| at_centre(image).is_none());
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn down_on_one_result_and_up_on_another_starts_nothing() {
    let (mut shown, [first, second]) = two_results();
    let output = shown.output.clone();
    shown.input.move_pointer(&output, first.0, first.1).unwrap();
    shown.input.press_button(Button::PRIMARY).unwrap();
    shown
        .input
        .move_pointer(&output, second.0, second.1)
        .unwrap();
    shown.input.release_button(Button::PRIMARY).unwrap();
    assert!(
        stays(|| shown.started().is_empty()),
        "{:?}",
        shown.started()
    );
    let image = shown.see("the launcher still shown", |image| {
        at_centre(image).is_some()
    });
    assert_eq!(results(&image).len(), 2, "its results still there");
}

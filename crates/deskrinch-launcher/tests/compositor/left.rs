//! The launcher left: another client's surface takes the keyboard from it
//! while it is shown. The test client's overlay (`deskrinch-test-client`'s
//! `TestLayer`, whose doc has what each compositor does) is that surface:
//! an overlay layer surface with the keyboard exclusively takes it from the
//! launcher, itself one, under KWin, sway and labwc alike.

use deskrinch_launcher::HAS_KEYBOARD;
use deskrinch_test_client::TestClient;
use deskrinch_test_input::Key;

use crate::screen::{Shown, args, at_centre, results_drawn, the_one_started};
use crate::support::stays;

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn another_surface_taking_the_keyboard_hides_it_and_starts_nothing() {
    let mut shown = Shown::start("");
    shown.type_query("plain");
    shown.see("the results of the query", results_drawn);

    let client = TestClient::connect().unwrap();
    let overlay = client.open_overlay().unwrap();
    overlay.wait_keyboard().unwrap();
    shown.see("the launcher hidden", |image| at_centre(image).is_none());
    // Its query counted at the hide, as at any; the keyboard said once.
    shown.hid_with_the_query();
    assert!(
        stays(|| shown.started().is_empty()),
        "{:?}",
        shown.started()
    );

    // The keyboard comes back to nothing: the launcher stays hidden.
    let mark = shown.launcher.mark();
    let frames = shown.launcher.frames();
    drop(overlay);
    assert!(
        stays(|| shown.launcher.frames() == frames),
        "the launcher presented again: {} frames, then {}",
        frames,
        shown.launcher.frames()
    );
    let since = shown.launcher.lines_since(mark);
    assert!(
        !since.iter().any(|line| line.contains(HAS_KEYBOARD)),
        "the launcher took the keyboard back: {since:#?}"
    );

    // Shown again, the keyboard is in its field and it works.
    shown.show_again();
    shown.type_query("plain");
    shown.see("the results of the query", results_drawn);
    shown.input.press(Key::ENTER).unwrap();
    assert_eq!(args(&the_one_started(&shown)), ["Plain"]);
    shown.hid_with_the_query();
}

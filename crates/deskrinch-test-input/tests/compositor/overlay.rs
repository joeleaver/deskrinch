//! The test client's overlay (`deskrinch-test-client`'s `TestLayer`): it
//! takes the keyboard from a window of another client, and gives it up
//! when it goes. Here, beside the windows' keyboard tests, because the seat
//! of a headless wlroots compositor has a keyboard only once this crate has
//! made one.

use deskrinch_test_client::{Compositor, TestClient};
use deskrinch_test_input::{Input, Key};
use deskrinch_test_wait::comes_true;

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_overlay_takes_the_keyboard_from_a_window_and_gives_it_back_when_it_goes() {
    Compositor::current();
    let mut input = Input::connect().unwrap();
    let windows = TestClient::connect().unwrap();
    let window = windows.open("under", "org.deskrinch.InputTest").unwrap();
    window.wait_keyboard().unwrap();

    let overlays = TestClient::connect().unwrap();
    let overlay = overlays.open_overlay().unwrap();
    overlay.wait_keyboard().unwrap();
    assert!(
        comes_true(|| !window.has_keyboard()),
        "the window kept the keyboard"
    );
    input.press(Key::ESCAPE).unwrap();

    drop(overlay);
    window.wait_keyboard().unwrap();
    input.press(Key::TAB).unwrap();
    window.wait_key(Key::TAB.code()).unwrap();
    assert_eq!(
        window.keys(),
        [Key::TAB.code()],
        "only the key pressed after the overlay went"
    );
}

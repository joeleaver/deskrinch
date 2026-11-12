//! Keys typed into a window of the test's own (`deskrinch-test-client`).

use deskrinch_test_client::{Compositor, TestClient};
use deskrinch_test_input::{Error, Input, Key};

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn keys_typed_reach_the_window_with_the_focus_in_order() {
    Compositor::current();
    // First: on a headless wlroots seat it is the seat's keyboard.
    let mut input = Input::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let window = client
        .open("typed into", "org.deskrinch.InputTest")
        .unwrap();
    window.wait_keyboard().unwrap();
    input.type_text("ok 1").unwrap();
    input.press(Key::ENTER).unwrap();
    window.wait_key(Key::ENTER.code()).unwrap();
    // KEY_O, KEY_K, KEY_SPACE, KEY_1, KEY_ENTER.
    assert_eq!(window.keys(), [24, 37, 57, 2, 28]);
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_chord_holds_its_modifiers_around_its_key() {
    Compositor::current();
    let mut input = Input::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let window = client.open("chorded", "org.deskrinch.InputTest").unwrap();
    window.wait_keyboard().unwrap();
    // Not a modifier: nothing is sent.
    assert!(matches!(
        input.press_with(&[Key::ESCAPE], Key::TAB),
        Err(Error::NotAModifier(Key::ESCAPE))
    ));
    input
        .press_with(&[Key::LEFT_CTRL, Key::LEFT_ALT], Key::TAB)
        .unwrap();
    input.press(Key::ENTER).unwrap();
    window.wait_key(Key::ENTER.code()).unwrap();
    // Each key pressed once, in order.
    assert_eq!(
        window.keys(),
        [
            Key::LEFT_CTRL.code(),
            Key::LEFT_ALT.code(),
            Key::TAB.code(),
            Key::ENTER.code()
        ]
    );
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn keys_go_to_the_window_with_the_focus_only() {
    Compositor::current();
    let mut input = Input::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let first = client.open("first", "org.deskrinch.InputTest").unwrap();
    first.wait_keyboard().unwrap();
    let second = client.open("second", "org.deskrinch.InputTest").unwrap();
    // A new window takes the focus on all three compositors.
    second.wait_keyboard().unwrap();
    input.press(Key::ESCAPE).unwrap();
    second.wait_key(Key::ESCAPE.code()).unwrap();
    assert!(!first.has_keyboard());
    assert_eq!(first.keys(), Vec::<u32>::new());
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn text_with_an_untypable_character_types_nothing() {
    Compositor::current();
    let mut input = Input::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let window = client.open("untyped", "org.deskrinch.InputTest").unwrap();
    window.wait_keyboard().unwrap();
    assert!(matches!(input.type_text("aB"), Err(Error::Untypable('B'))));
    input.press(Key::TAB).unwrap();
    window.wait_key(Key::TAB.code()).unwrap();
    assert_eq!(
        window.keys(),
        [Key::TAB.code()],
        "nothing of \"aB\" was typed"
    );
}

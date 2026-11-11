//! The pointer reaches the window under a point on a named output, at that
//! point in the window's own coordinates, and presses and releases its
//! buttons there. Each window is fullscreen on its output, so its surface
//! coordinates are the output's logical pixels: a point at `(x, y)` on the
//! output must reach it at `(x, y)`, on every output (a wrong offset in the
//! layout lands on the wrong one), at every scale and transform.

use deskrinch_test_client::{Compositor, Paint, PointerEvent, TestClient, TestWindow};
use deskrinch_test_input::{Button, Error, Input};
use deskrinch_test_wayland::Rgba;

const PRIMARY: u32 = 272;

/// A window fullscreen on each output, by output name.
fn fullscreen_on_each(client: &TestClient) -> Vec<(String, TestWindow, (i32, i32))> {
    client
        .output_names()
        .into_iter()
        .map(|name| {
            let paint = Paint::Solid(Rgba::opaque(40, 90, 160));
            let window = client
                .open_painted(
                    &format!("on {name}"),
                    "org.deskrinch.PointerTest",
                    paint,
                    Some(&name),
                )
                .unwrap();
            let size = window.wait_fullscreen().unwrap();
            (name, window, size)
        })
        .collect()
}

/// The points tried on an output of `size`: near its top-left corner, and
/// near its bottom-right one.
fn points((width, height): (i32, i32)) -> [(f64, f64); 2] {
    [
        (100.0, 80.0),
        (f64::from(width) - 40.0, f64::from(height) - 30.0),
    ]
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn the_pointer_reaches_the_window_at_the_point_on_every_output() {
    Compositor::current();
    // First: on a headless wlroots seat it is the seat's pointer.
    let mut input = Input::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let windows = fullscreen_on_each(&client);
    assert!(windows.len() > 1, "the sessions have more than one output");
    for (name, window, size) in &windows {
        for (x, y) in points(*size) {
            input.move_pointer(name, x, y).unwrap();
            window.wait_pointer_at(x, y).unwrap_or_else(|error| {
                panic!(
                    "{name} at ({x}, {y}): {error}: {:?}",
                    window.pointer_events()
                )
            });
        }
    }
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_click_presses_and_releases_the_button_where_the_pointer_is() {
    Compositor::current();
    let mut input = Input::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let windows = fullscreen_on_each(&client);
    let (name, window, size) = windows.last().unwrap();
    let (x, y) = points(*size)[0];
    input.move_pointer(name, x, y).unwrap();
    window.wait_pointer_at(x, y).unwrap();
    input.click(Button::PRIMARY).unwrap();
    window
        .wait_pointer("a press and a release", |events| {
            events.ends_with(&[
                PointerEvent::Pressed { button: PRIMARY },
                PointerEvent::Released { button: PRIMARY },
            ])
        })
        .unwrap_or_else(|error| panic!("{error}: {:?}", window.pointer_events()));
    assert_eq!(window.pointer_position(), Some((x, y)), "it did not move");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_button_held_is_released_apart() {
    Compositor::current();
    let mut input = Input::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let windows = fullscreen_on_each(&client);
    let (name, window, size) = &windows[0];
    let (x, y) = points(*size)[1];
    input.move_pointer(name, x, y).unwrap();
    window.wait_pointer_at(x, y).unwrap();
    input.press_button(Button::SECONDARY).unwrap();
    window
        .wait_pointer("the secondary button down", |events| {
            events.last() == Some(&PointerEvent::Pressed { button: 273 })
        })
        .unwrap();
    input.release_button(Button::SECONDARY).unwrap();
    window
        .wait_pointer("the secondary button up", |events| {
            events.last() == Some(&PointerEvent::Released { button: 273 })
        })
        .unwrap();
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_point_off_its_output_or_on_no_output_is_refused() {
    Compositor::current();
    let mut input = Input::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let name = client.output_names().remove(0);
    let (width, _) = client.output_info(&name).unwrap().logical_size.unwrap();
    let off = input.move_pointer(&name, f64::from(width), 0.0);
    assert!(matches!(off, Err(Error::OffOutput { .. })), "{off:?}");
    let none = input.move_pointer("NO-SUCH-OUTPUT", 0.0, 0.0);
    assert!(matches!(none, Err(Error::NoOutput(_))), "{none:?}");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_window_opened_before_the_pointer_hears_it_too() {
    Compositor::current();
    let client = TestClient::connect().unwrap();
    let windows = fullscreen_on_each(&client);
    let mut input = Input::connect().unwrap();
    let (name, window, size) = &windows[0];
    let (x, y) = points(*size)[0];
    input.move_pointer(name, x, y).unwrap();
    window
        .wait_pointer_at(x, y)
        .unwrap_or_else(|error| panic!("{error}: {:?}", window.pointer_events()));
}

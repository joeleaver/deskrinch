//! The pointer reaches the window under a point on a named output, at that
//! point in the window's own coordinates, and presses and releases its
//! buttons there. Each window is fullscreen on its output, so its surface
//! coordinates are the output's logical pixels: a point at `(x, y)` on the
//! output must reach it at `(x, y)`, on every output (a wrong offset in the
//! layout lands on the wrong one), at every scale and transform.
//!
//! What the tests wait for is always something they caused: a window heard
//! the pointer at a point *after* a mark taken before the move ([`reach`]).
//! A compositor keeps the cursor, and its focus, where the last test left
//! it, and a window maps under it; a seat may also take its pointer away
//! and give it back as devices come and go (labwc does, as a virtual
//! pointer is made). A position heard before the move proves nothing, and a
//! press sent then may reach no window.

use deskrinch_test_client::{Compositor, Paint, PointerEvent, TestClient, TestWindow};
use deskrinch_test_input::{Button, Error, Input};
use deskrinch_test_wayland::Rgba;

const PRIMARY: u32 = 272;
const SECONDARY: u32 = 273;

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

/// Moves the pointer to `(x, y)` on `output` and waits until `window` has
/// heard it come there since: a pixel beside the point first, so that the
/// move is a motion the window hears even when the cursor was already at
/// the point.
fn reach(input: &mut Input, window: &TestWindow, output: &str, (x, y): (f64, f64)) {
    let mark = window.pointer_mark();
    input.move_pointer(output, x + 1.0, y).unwrap();
    input.move_pointer(output, x, y).unwrap();
    window
        .wait_pointer_at_since(mark, x, y)
        .unwrap_or_else(|error| panic!("{error}: {:?}", window.pointer_events()));
}

/// The presses and releases among `events`.
fn buttons(events: &[PointerEvent]) -> Vec<PointerEvent> {
    events
        .iter()
        .copied()
        .filter(|event| {
            matches!(
                event,
                PointerEvent::Pressed { .. } | PointerEvent::Released { .. }
            )
        })
        .collect()
}

/// Waits until `window` has heard, since the `from`th event, exactly
/// `expected` of the buttons; panics when it does not.
fn wait_buttons(window: &TestWindow, from: usize, expected: &[PointerEvent]) {
    window
        .wait_pointer(&format!("{expected:?}"), |events| {
            buttons(&events[from..]) == expected
        })
        .unwrap_or_else(|error| panic!("{error}: {:?}", window.pointer_events()));
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
        for point in points(*size) {
            reach(&mut input, window, name, point);
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
    let point = points(*size)[0];
    reach(&mut input, window, name, point);
    let from = window.pointer_events().len();
    input.click(Button::PRIMARY).unwrap();
    wait_buttons(
        window,
        from,
        &[
            PointerEvent::Pressed { button: PRIMARY },
            PointerEvent::Released { button: PRIMARY },
        ],
    );
    assert_eq!(window.pointer_position(), Some(point), "it did not move");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_button_held_is_released_apart() {
    Compositor::current();
    let mut input = Input::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let windows = fullscreen_on_each(&client);
    let (name, window, size) = &windows[0];
    reach(&mut input, window, name, points(*size)[1]);
    let from = window.pointer_events().len();
    let pressed = PointerEvent::Pressed { button: SECONDARY };
    input.press_button(Button::SECONDARY).unwrap();
    wait_buttons(window, from, &[pressed]);
    input.release_button(Button::SECONDARY).unwrap();
    wait_buttons(
        window,
        from,
        &[pressed, PointerEvent::Released { button: SECONDARY }],
    );
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
    reach(&mut input, window, name, points(*size)[0]);
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_input_dropped_with_a_button_down_releases_it() {
    Compositor::current();
    let client = TestClient::connect().unwrap();
    let windows = fullscreen_on_each(&client);
    let (name, window, size) = &windows[0];
    let [first, second] = points(*size);
    let pressed = PointerEvent::Pressed { button: PRIMARY };
    let released = PointerEvent::Released { button: PRIMARY };
    let from = {
        let mut held = Input::connect().unwrap();
        reach(&mut held, window, name, first);
        let from = window.pointer_events().len();
        held.press_button(Button::PRIMARY).unwrap();
        wait_buttons(window, from, &[pressed]);
        from
        // Dropped here, with the button still down.
    };
    wait_buttons(window, from, &[pressed, released]);
    let from = window.pointer_events().len();
    let mut input = Input::connect().unwrap();
    reach(&mut input, window, name, second);
    input.click(Button::PRIMARY).unwrap();
    // A move after the click: what the compositor sent for the click has
    // all arrived once the window hears the pointer at the new point.
    reach(&mut input, window, name, first);
    assert_eq!(
        buttons(&window.pointer_events()[from..]),
        [pressed, released],
        "exactly one press and one release: {:?}",
        window.pointer_events()
    );
}

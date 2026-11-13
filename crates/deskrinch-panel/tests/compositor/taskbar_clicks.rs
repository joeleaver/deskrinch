//! The taskbar against real compositors, on [`crate::taskbar`]'s fixture:
//! a button for each window, which follows its window's title and goes with
//! it; a click of the primary button activates its window, or minimizes the
//! active one, and a second restores it; the middle button asks it to
//! close; and when even the icons do not fit, the oldest windows are left
//! out but never the active one.
//!
//! Which window is active is what the test client's windows are told (the
//! keyboard focus); which is minimized, what the panel draws (the button
//! dimmed), where the compositor has a minimized state: sway has none, and
//! ignores the request.

use std::time::{Duration, Instant};

use deskrinch_test_client::{Compositor, TestWindow};
use deskrinch_test_input::Button;
use deskrinch_test_screen::Image;
use deskrinch_window_management::{Action, WindowManagement};

use crate::support::PATIENCE;
use crate::taskbar::{App, Bar, GREEN, MAGENTA, RED, window_of};

/// Opens a window of `app` and makes it the active one, through a
/// window-management connection of the test's own: which window a
/// compositor activates on opening is its own policy.
fn open_active(bar: &Bar, app: App, title: &str) -> TestWindow {
    let window = bar.client.open(title, app.id).unwrap();
    let (manager, id) = window_of(app.id);
    manager.perform(id, Action::Activate).unwrap();
    window.wait_keyboard().unwrap();
    window
}

/// Waits until `done`, checked every 50 ms; panics naming `what` when it
/// is not so within [`PATIENCE`].
fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "{what} did not happen");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_click_activates_the_window_of_the_button() {
    let mut bar = Bar::start();
    let red = open_active(&bar, RED, "Red");
    let green = open_active(&bar, GREEN, "Green");
    let red_icon = bar.wait_button(RED);
    bar.click(red_icon, Button::PRIMARY);
    red.wait_keyboard().unwrap();
    let green_icon = bar.wait_button(GREEN);
    bar.click(green_icon, Button::PRIMARY);
    green.wait_keyboard().unwrap();
    assert!(!red.has_keyboard(), "one active window");
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_click_on_the_active_window_minimizes_it_and_another_restores_it() {
    let compositor = Compositor::current();
    let mut bar = Bar::start();
    let red = open_active(&bar, RED, "Red");
    let icon = bar.wait_button(RED);
    bar.click(icon, Button::PRIMARY);
    if compositor == Compositor::Sway {
        // sway accepts the request and has no state to put the window in.
        return;
    }
    bar.see("the button dimmed", |_, image| {
        Bar::dimmed(image, RED, icon)
    });
    bar.click(icon, Button::PRIMARY);
    bar.see("the button at full colour", |bar, image| {
        bar.icon(image, RED).is_some()
    });
    red.wait_keyboard().unwrap();
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_middle_click_asks_the_window_to_close() {
    let mut bar = Bar::start();
    let red = open_active(&bar, RED, "Red");
    let green = open_active(&bar, GREEN, "Green");
    let icon = bar.wait_button(RED);
    bar.click(icon, Button::MIDDLE);
    wait_until("the close request", || red.close_requested());
    assert!(!green.close_requested(), "only the window clicked");
    // The window obeyed, as an application would, without breaking the
    // protocol: its connection still opens windows.
    drop(red);
    bar.client.open("Red again", RED.id).unwrap();
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_button_follows_its_window_title_and_goes_with_it() {
    let mut bar = Bar::start();
    let _red = open_active(&bar, RED, "Red");
    let magenta = open_active(&bar, MAGENTA, "A short title");
    let icon = bar.wait_button(MAGENTA);
    let before = bar.settled(|image| title_of(image, icon));
    magenta
        .set_title("Quite another title, and a longer one")
        .unwrap();
    bar.see("the new title", |_, image| title_of(image, icon) != before);
    drop(magenta);
    bar.see("the button gone", |bar, image| {
        bar.icon(image, MAGENTA).is_none() && bar.icon(image, RED).is_some()
    });
}

/// The pixels of a titled button right of its icon (`icon`, a box): where
/// its title is drawn.
fn title_of(image: &Image, icon: (u32, u32, u32, u32)) -> Vec<Option<deskrinch_test_screen::Rgba>> {
    let (_, top, right, bottom) = icon;
    let side = bottom - top + 1;
    let mut pixels = Vec::new();
    for y in top..=bottom {
        for x in right + side / 4..right + side * 4 {
            pixels.push(image.pixel(x, y));
        }
    }
    pixels
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_oldest_windows_are_left_out_but_never_the_active_one() {
    let mut bar = Bar::start();
    let _green = bar.client.open("Green", GREEN.id).unwrap();
    bar.wait_button(GREEN);
    // More icons than the output is wide: a button's icon is 28 pixels
    // wide with its padding, 32 with the gap after it.
    let many = bar.logical_width() / 32 + 4;
    let _reds: Vec<TestWindow> = (0..many)
        .map(|n| bar.client.open(&format!("Red {n}"), RED.id).unwrap())
        .collect();
    // A red window active, whichever the compositor activated on opening.
    let (manager, red) = window_of(RED.id);
    manager.perform(red, Action::Activate).unwrap();
    bar.see("the oldest left out", |bar, image| {
        bar.icon(image, GREEN).is_none() && bar.icons(image, RED) > 1
    });
    let (manager, green) = window_of(GREEN.id);
    manager.perform(green, Action::Activate).unwrap();
    bar.see("the active window shown", |bar, image| {
        bar.icon(image, GREEN).is_some()
    });
}

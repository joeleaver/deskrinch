//! Maximized, fullscreen, and which outputs a window is on. The session has
//! two outputs side by side.

use deskrinch_window_management::{Window, WindowState};

use crate::support::{Behaviour, Compositor, Watcher, unique_app_id};
use deskrinch_test_client::TestClient;

fn has(state: WindowState) -> impl Fn(&Window) -> bool {
    move |window| window.state.contains(state)
}

fn lacks(state: WindowState) -> impl Fn(&Window) -> bool {
    move |window| !window.state.contains(state)
}

/// Whether `window` is on `output` and no other.
fn alone_on(window: &Window, output: &str) -> bool {
    window.outputs.len() == 1 && window.outputs.contains(output)
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn reports_maximized_when_the_window_asks() {
    let compositor = Compositor::current();
    let app_id = unique_app_id("maximize");
    let watcher = Watcher::connect();
    let client = TestClient::connect().unwrap();
    let window = client.open("Maximize me", &app_id).unwrap();
    watcher.wait_window("the window", &app_id, lacks(WindowState::MAXIMIZED));

    window.set_maximized(true).unwrap();
    if !compositor.maximizes() {
        return;
    }
    watcher.wait_window("maximized", &app_id, has(WindowState::MAXIMIZED));
    window.set_maximized(false).unwrap();
    watcher.wait_window("unmaximized", &app_id, lacks(WindowState::MAXIMIZED));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn reports_fullscreen_when_the_window_asks() {
    Compositor::current();
    let app_id = unique_app_id("fullscreen");
    let watcher = Watcher::connect();
    let client = TestClient::connect().unwrap();
    let window = client.open("Fullscreen me", &app_id).unwrap();
    watcher.wait_window("the window", &app_id, lacks(WindowState::FULLSCREEN));

    window.set_fullscreen(true, None).unwrap();
    watcher.wait_window("fullscreen", &app_id, has(WindowState::FULLSCREEN));
    window.set_fullscreen(false, None).unwrap();
    watcher.wait_window("windowed", &app_id, lacks(WindowState::FULLSCREEN));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn reports_the_outputs_a_window_is_on() {
    let compositor = Compositor::current();
    let app_id = unique_app_id("outputs");
    let watcher = Watcher::connect();
    let client = TestClient::connect().unwrap();
    let outputs = client.output_names();
    assert_eq!(outputs.len(), 2, "the session has two outputs: {outputs:?}");
    let window = client.open("Travelling", &app_id).unwrap();

    // Placed on one output or the other, by the compositor's choice.
    let placed = watcher.wait_window("the window placed", &app_id, |window| {
        !window.outputs.is_empty()
    });
    assert!(
        placed.outputs.iter().all(|name| outputs.contains(name)),
        "{placed:?}"
    );

    // Fullscreen on each output in turn: then it is on that one alone. (Each
    // time from windowed: a compositor need not move a window that is
    // already fullscreen.)
    for output in outputs.iter().rev().chain(&outputs) {
        let what = format!("the window on {output} alone");
        window.set_fullscreen(true, Some(output)).unwrap();
        if compositor.tracks_fullscreen_outputs() {
            watcher.wait_window(&what, &app_id, |window| {
                has(WindowState::FULLSCREEN)(window) && alone_on(window, output)
            });
            window.set_fullscreen(false, None).unwrap();
            watcher.wait_window("windowed", &app_id, lacks(WindowState::FULLSCREEN));
        } else {
            // The compositor moved it there; it reports so once windowed.
            watcher.wait_window("fullscreen", &app_id, has(WindowState::FULLSCREEN));
            window.set_fullscreen(false, None).unwrap();
            watcher.wait_window(&what, &app_id, |window| {
                lacks(WindowState::FULLSCREEN)(window) && alone_on(window, output)
            });
        }
    }
}

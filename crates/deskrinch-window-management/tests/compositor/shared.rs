//! Managers sharing one connection (`connect_with`), each on a queue of its
//! own, both dispatched by the client's one thread: both hear the
//! compositor, and one going away leaves the other working, and tells the
//! compositor to stop.

use std::thread;
use std::time::{Duration, Instant};

use deskrinch_test_client::TestClient;
use deskrinch_window_management::{Client, StateCounts, WindowManagement};

use crate::support::{Compositor, Watcher, unique_app_id};
use deskrinch_test_wait::PATIENCE;

/// Waits until `client`'s states are as `wanted`, ignoring the events
/// discarded; panics with what they are otherwise.
fn wait_states(client: &Client, attached: usize, releasing: usize) -> StateCounts {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let states = client.states();
        if (states.attached, states.releasing) == (attached, releasing) {
            return states;
        }
        assert!(
            Instant::now() < deadline,
            "{attached} attached and {releasing} releasing wanted: {states:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn managers_on_one_connection_both_hear_every_window() {
    Compositor::current();
    let client = Client::connect().unwrap();
    let first = Watcher::connect_with(&client);
    let second = Watcher::connect_with(&client);
    let app_id = unique_app_id("shared");
    let test = TestClient::connect().unwrap();
    let window = test.open("Shared", &app_id).unwrap();
    let seen_first = first.wait_window("the window, by the first", &app_id, |_| true);
    let seen_second = second.wait_window("the window, by the second", &app_id, |_| true);
    assert_eq!(seen_first.title, seen_second.title);
    drop(window);
    first.wait_gone("the window gone, for the first", &app_id);
    second.wait_gone("the window gone, for the second", &app_id);
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn dropping_one_manager_leaves_the_other_on_the_connection() {
    let compositor = Compositor::current();
    let client = Client::connect().unwrap();
    let kept = Watcher::connect_with(&client);
    drop(Watcher::connect_with(&client));
    match compositor {
        // Stopped, `finished` heard, every handle destroyed: gone.
        Compositor::Sway | Compositor::Labwc => wait_states(&client, 1, 0),
        // KWin's manager has no destructor: kept, its events thrown away.
        Compositor::Kwin => wait_states(&client, 1, 1),
    };
    let app_id = unique_app_id("survivor");
    let test = TestClient::connect().unwrap();
    let _window = test.open("Survivor", &app_id).unwrap();
    kept.wait_window("the window, after the other manager went", &app_id, |_| {
        true
    });
    assert!(
        kept.manager
            .windows()
            .iter()
            .any(|window| window.app_id == app_id)
    );
}

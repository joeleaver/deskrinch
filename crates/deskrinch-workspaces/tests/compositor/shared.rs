//! States on one shared connection, each on a queue of its own, all
//! dispatched by the client's one thread: workspaces beside window
//! management, and a workspace manager dropped beside another, which tells
//! the compositor to stop.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_test_client::TestClient;
use deskrinch_window_management::WindowManagement;
use deskrinch_workspaces::{Client, StateCounts, Workspaces};

use crate::support::{Compositor, PATIENCE, Watcher, compositor, kwin, set_up};

/// Switches made under KWin after a manager is dropped.
const SWITCHES: u64 = 20;
/// What KWin sends a desktop manager's client for each switch (KWin 6.6.6,
/// traced): `deactivated` on the desktop left, `activated` on the one
/// switched to, and the manager's `done`.
const EVENTS_PER_SWITCH: u64 = 3;

/// Waits until `check` holds for `client`'s state counts; panics with them
/// otherwise.
fn wait_states(client: &Client, what: &str, check: impl Fn(StateCounts) -> bool) -> StateCounts {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let states = client.states();
        if check(states) {
            return states;
        }
        assert!(Instant::now() < deadline, "{what}: {states:?}");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn workspaces_and_windows_share_one_connection() {
    let compositor = compositor();
    let client = Client::connect().unwrap();
    let watcher = Watcher::connect_with(&client);
    let (sender, windows) = mpsc::channel();
    let manager = deskrinch_window_management::connect_with(&client, move |event| {
        if let deskrinch_window_management::Event::Added(window) = event {
            // The receiver outlives the manager.
            sender.send(window.app_id).unwrap();
        }
    })
    .expect("window management on the shared connection");
    set_up(compositor, &watcher);

    // Window management hears a new window.
    let app_id = format!("org.deskrinch.test.Shared.p{}", std::process::id());
    let test = TestClient::connect().unwrap();
    let _window = test.open("Shared", &app_id).unwrap();
    let deadline = Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let added = windows
            .recv_timeout(left.max(Duration::from_millis(1)))
            .expect("the window, through window management");
        if added == app_id {
            break;
        }
    }
    assert!(
        manager
            .windows()
            .iter()
            .any(|window| window.app_id == app_id)
    );

    // And workspaces still switch on the same connection.
    let target = watcher.inactive();
    watcher.activate(&target.name);
    assert!(
        watcher
            .manager
            .workspaces()
            .iter()
            .any(|workspace| workspace.id == target.id)
    );
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_dropped_manager_stops_while_the_other_hears_on() {
    let compositor = compositor();
    let client = Client::connect().unwrap();
    let kept = Watcher::connect_with(&client);
    let dropped = Watcher::connect_with(&client);
    set_up(compositor, &kept);
    assert_eq!(client.states().attached, 2);
    drop(dropped);
    match compositor {
        // Every handle destroyed, the manager stopped and its `finished`
        // heard: nothing reaches the state, which is gone.
        Compositor::Labwc => {
            wait_states(&client, "the dropped state gone", |states| {
                (states.attached, states.releasing) == (1, 0)
            });
        }
        // KWin's desktops have no destructor: the state is kept, and what
        // KWin goes on sending it is consumed as it comes, not kept.
        Compositor::Kwin => {
            let before = wait_states(&client, "the dropped state releasing", |states| {
                (states.attached, states.releasing) == (1, 1)
            });
            for _ in 0..SWITCHES {
                let target = kept.inactive();
                kwin::set_current(&target.name);
                kept.wait_active(&target.name);
            }
            let wanted = before.discarded + SWITCHES * EVENTS_PER_SWITCH;
            let after = wait_states(
                &client,
                &format!(
                    "at least {EVENTS_PER_SWITCH} events discarded per switch, \
                     {SWITCHES} switches, from {} (at least {wanted})",
                    before.discarded
                ),
                |states| states.discarded >= wanted,
            );
            assert_eq!(after.releasing, 1, "{after:?}");
            println!(
                "{SWITCHES} switches: {} events discarded ({} to {})",
                after.discarded - before.discarded,
                before.discarded,
                after.discarded
            );
        }
        Compositor::Sway => panic!("sway offers no workspace protocol"),
    }
    // The other manager hears its own, and switches.
    let target = kept.inactive();
    kept.activate(&target.name);
}

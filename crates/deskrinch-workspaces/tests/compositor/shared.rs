//! Workspaces and window management on one shared connection: each on a
//! queue of its own, both dispatched by the client's one thread.

use std::sync::mpsc;
use std::time::Duration;

use deskrinch_test_client::TestClient;
use deskrinch_window_management::WindowManagement;
use deskrinch_workspaces::{Client, Workspaces};

use crate::support::{PATIENCE, Watcher, compositor, set_up};

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
    let deadline = std::time::Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
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

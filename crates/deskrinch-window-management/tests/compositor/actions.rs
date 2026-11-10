//! Actions act on the window asked for, and only on it.

use deskrinch_window_management::{Action, ActionError, Window, WindowManagement, WindowState};

use crate::support::test_window::TestClient;
use crate::support::{Compositor, Watcher, find, unique_app_id};

fn has(state: WindowState) -> impl Fn(&Window) -> bool {
    move |window| window.state.contains(state)
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn activates_the_window_asked_for() {
    Compositor::current();
    let [one_id, two_id] = [unique_app_id("activate-one"), unique_app_id("activate-two")];
    let watcher = Watcher::connect();
    let client = TestClient::connect();
    let _one = client.open("One", &one_id);
    let _two = client.open("Two", &two_id);
    let one = watcher.wait_window("window one", &one_id, |_| true).id;
    let two = watcher.wait_window("window two", &two_id, |_| true).id;

    for (target, other) in [(one, two), (two, one), (one, two)] {
        watcher.manager.perform(target, Action::Activate).unwrap();
        watcher.wait_until("the activation", |windows| {
            let active = |id| {
                windows
                    .iter()
                    .find(|window| window.id == id)
                    .is_some_and(has(WindowState::ACTIVE))
            };
            (active(target) && !active(other)).then_some(())
        });
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn minimizes_and_restores_the_window_asked_for() {
    let compositor = Compositor::current();
    let [one_id, two_id] = [unique_app_id("minimize-one"), unique_app_id("minimize-two")];
    let watcher = Watcher::connect();
    let client = TestClient::connect();
    let _one = client.open("One", &one_id);
    let _two = client.open("Two", &two_id);
    let one = watcher.wait_window("window one", &one_id, |_| true).id;
    watcher.wait_window("window two", &two_id, |_| true);

    watcher.manager.perform(one, Action::Minimize).unwrap();
    if !compositor.minimizes() {
        // The request is accepted; the compositor has no state to put the
        // window in, so there is nothing further to observe.
        return;
    }
    watcher.wait_window("window one minimized", &one_id, has(WindowState::MINIMIZED));
    let two = find(&watcher.seen(), &two_id).cloned().expect("window two");
    assert!(
        !has(WindowState::MINIMIZED)(&two),
        "only the window asked for"
    );

    watcher.manager.perform(one, Action::Restore).unwrap();
    watcher.wait_window("window one restored", &one_id, |window| {
        !window.state.contains(WindowState::MINIMIZED)
    });
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn closes_the_window_asked_for() {
    Compositor::current();
    let [keep_id, close_id] = [unique_app_id("close-keep"), unique_app_id("close-close")];
    let watcher = Watcher::connect();
    let client = TestClient::connect();
    let keep = client.open("Keep", &keep_id);
    let close = client.open("Close", &close_id);
    watcher.wait_window("the window to keep", &keep_id, |_| true);
    let doomed = watcher
        .wait_window("the window to close", &close_id, |_| true)
        .id;

    watcher.manager.perform(doomed, Action::Close).unwrap();
    watcher.wait_gone("the closed window", &close_id);
    assert!(close.close_requested());
    assert!(!keep.close_requested());
    assert!(find(&watcher.seen(), &keep_id).is_some());

    // The window is gone, and so are actions on it.
    let error = watcher
        .manager
        .perform(doomed, Action::Activate)
        .unwrap_err();
    assert!(
        matches!(error, ActionError::UnknownWindow(id) if id == doomed),
        "{error:?}"
    );
}

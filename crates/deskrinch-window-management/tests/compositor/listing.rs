//! The list: what is in it, and how it follows the windows.

use deskrinch_window_management::{Action, WindowManagement, WindowState};

use crate::support::test_window::TestClient;
use crate::support::{Compositor, Watcher, find, unique_app_id};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn capabilities_match_the_compositor() {
    let compositor = Compositor::current();
    let watcher = Watcher::connect();
    let capabilities = watcher.manager.capabilities();
    assert_eq!(capabilities.protocol, compositor.protocol());
    assert!(
        Action::ALL
            .into_iter()
            .all(|action| capabilities.supports(action))
    );
    // Every compositor here names its outputs and reports parents: KWin's
    // outputs are worked out from geometry and the output layout.
    assert!(capabilities.outputs);
    assert!(capabilities.parent);
    let core = WindowState::ACTIVE
        | WindowState::MINIMIZED
        | WindowState::MAXIMIZED
        | WindowState::FULLSCREEN;
    assert!(capabilities.states.contains(core));
    let hints = WindowState::DEMANDS_ATTENTION | WindowState::SKIP_TASKBAR;
    match compositor {
        Compositor::Kwin => assert!(capabilities.states.contains(hints)),
        Compositor::Sway | Compositor::Labwc => {
            assert!(!capabilities.states.intersects(hints));
        }
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn lists_open_and_new_windows_with_title_and_app_id() {
    Compositor::current();
    let before_id = unique_app_id("before");
    let after_id = unique_app_id("after");
    let client = TestClient::connect();
    let _before = client.open("Opened before", &before_id);

    let watcher = Watcher::connect();
    // A window open before connecting is in the list, and was announced,
    // before `connect` returned.
    let listed = watcher.manager.windows();
    let window = find(&listed, &before_id).expect("the window open before connecting");
    assert_eq!(window.title, "Opened before");
    assert_eq!(window.parent, None);
    watcher.drain();
    assert_eq!(watcher.seen(), listed, "the events describe the same list");

    let _after = client.open("Opened after", &after_id);
    let window = watcher.wait_window("the new window", &after_id, |_| true);
    assert_eq!(window.title, "Opened after");
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn reports_title_changes() {
    Compositor::current();
    let app_id = unique_app_id("retitled");
    let watcher = Watcher::connect();
    let client = TestClient::connect();
    let window = client.open("First title", &app_id);
    watcher.wait_window("the window", &app_id, |window| {
        window.title == "First title"
    });

    window.set_title("Second title");
    watcher.wait_window("the new title", &app_id, |window| {
        window.title == "Second title"
    });
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_window_its_client_destroys_is_removed() {
    Compositor::current();
    let app_id = unique_app_id("unmapped");
    let watcher = Watcher::connect();
    let client = TestClient::connect();
    let window = client.open("Brief", &app_id);
    watcher.wait_window("the window", &app_id, |_| true);
    drop(window);
    watcher.wait_gone("the destroyed window", &app_id);
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn reports_a_child_windows_parent() {
    let compositor = Compositor::current();
    let [main_id, dialog_id] = [unique_app_id("parent"), unique_app_id("child")];
    let watcher = Watcher::connect();
    let client = TestClient::connect();
    let main = client.open("Main", &main_id);
    let dialog = client.open_child(&main, "Dialog", &dialog_id);

    let main_window = watcher.wait_window("the main window", &main_id, |_| true);
    assert_eq!(main_window.parent, None);
    if !compositor.reports_parents() {
        let dialog = watcher.wait_window("the dialog", &dialog_id, |_| true);
        assert_eq!(dialog.parent, None);
        return;
    }
    watcher.wait_window("the dialog's parent", &dialog_id, |window| {
        window.parent == Some(main_window.id)
    });

    // When the parent goes, the child no longer names it.
    drop(main);
    watcher.wait_gone("the main window", &main_id);
    watcher.wait_window("the orphaned dialog", &dialog_id, |window| {
        window.parent.is_none()
    });
    drop(dialog);
}

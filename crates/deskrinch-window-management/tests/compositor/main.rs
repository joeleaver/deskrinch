//! The crate against a real compositor: nested KWin or sway, started by
//! `cargo xtask compositor-tests`, which runs these one at a time (they share
//! the compositor's focus). Each test opens its own windows, each a separate
//! client, and finds them by an app id no other window has.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
#[path = "../support/mod.rs"]
mod support;

use deskrinch_window_management::{Action, ActionError, Protocol, WindowManagement, WindowState};
use support::test_window::TestWindow;
use support::{Compositor, Watcher, find, unique_app_id};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn capabilities_match_the_compositor() {
    let compositor = Compositor::current();
    let watcher = Watcher::connect();
    let capabilities = watcher.manager.capabilities();
    let all = [
        Action::Activate,
        Action::Minimize,
        Action::Restore,
        Action::Close,
    ];
    assert!(all.into_iter().all(|action| capabilities.supports(action)));
    match compositor {
        Compositor::Kwin => {
            assert_eq!(capabilities.protocol, Protocol::PlasmaWindowManagement);
            assert!(!capabilities.outputs);
            assert_eq!(capabilities.states, WindowState::all());
        }
        Compositor::Sway => {
            assert_eq!(capabilities.protocol, Protocol::WlrForeignToplevel);
            assert!(capabilities.outputs);
            assert!(!capabilities.states.contains(WindowState::SKIP_TASKBAR));
            assert!(capabilities.states.contains(WindowState::FULLSCREEN));
        }
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn lists_open_and_new_windows_with_title_and_app_id() {
    let compositor = Compositor::current();
    let before_id = unique_app_id("before");
    let after_id = unique_app_id("after");
    let _before = TestWindow::open("Opened before", &before_id);

    let watcher = Watcher::connect();
    // A window open before connecting is in the list, and was announced,
    // before `connect` returned.
    let listed = watcher.manager.windows();
    let window = find(&listed, &before_id).expect("the window open before connecting");
    assert_eq!(window.title, "Opened before");
    watcher.drain();
    assert_eq!(watcher.seen(), listed, "the events describe the same list");

    let _after = TestWindow::open("Opened after", &after_id);
    let window = watcher.wait_window("the new window", &after_id, |_| true);
    assert_eq!(window.title, "Opened after");

    let window = watcher.wait_window("the new window's output", &after_id, |window| {
        compositor == Compositor::Kwin || !window.outputs.is_empty()
    });
    match compositor {
        // KWin's protocol does not say which output a window is on.
        Compositor::Kwin => assert!(window.outputs.is_empty()),
        Compositor::Sway => assert_eq!(window.outputs.iter().collect::<Vec<_>>(), ["HEADLESS-1"]),
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn reports_title_changes() {
    Compositor::current();
    let app_id = unique_app_id("retitled");
    let watcher = Watcher::connect();
    let test_window = TestWindow::open("First title", &app_id);
    watcher.wait_window("the window", &app_id, |window| {
        window.title == "First title"
    });

    test_window.set_title("Second title");
    watcher.wait_window("the new title", &app_id, |window| {
        window.title == "Second title"
    });
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn activates_the_window_asked_for() {
    Compositor::current();
    let [one_id, two_id] = [unique_app_id("activate-one"), unique_app_id("activate-two")];
    let watcher = Watcher::connect();
    let _one = TestWindow::open("One", &one_id);
    let _two = TestWindow::open("Two", &two_id);
    let one = watcher.wait_window("window one", &one_id, |_| true).id;
    let two = watcher.wait_window("window two", &two_id, |_| true).id;

    for (target, other) in [(one, two), (two, one), (one, two)] {
        watcher.manager.perform(target, Action::Activate).unwrap();
        watcher.wait_until("the activation", |windows| {
            let active = |id| {
                windows
                    .iter()
                    .find(|window| window.id == id)
                    .is_some_and(|window| window.state.contains(WindowState::ACTIVE))
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
    let _one = TestWindow::open("One", &one_id);
    let _two = TestWindow::open("Two", &two_id);
    let one = watcher.wait_window("window one", &one_id, |_| true).id;
    watcher.wait_window("window two", &two_id, |_| true);

    watcher.manager.perform(one, Action::Minimize).unwrap();
    if compositor == Compositor::Sway {
        // sway has no minimized state: it accepts the request and ignores it
        // (it handles only activate, fullscreen and close from this
        // protocol), so there is nothing further to observe.
        return;
    }
    let minimized = |window: &deskrinch_window_management::Window| {
        window.state.contains(WindowState::MINIMIZED)
    };
    watcher.wait_window("window one minimized", &one_id, minimized);
    let two = find(&watcher.seen(), &two_id).cloned().expect("window two");
    assert!(!minimized(&two), "only the window asked for is minimized");

    watcher.manager.perform(one, Action::Restore).unwrap();
    watcher.wait_window("window one restored", &one_id, |window| !minimized(window));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn closes_the_window_asked_for() {
    Compositor::current();
    let [keep_id, close_id] = [unique_app_id("close-keep"), unique_app_id("close-close")];
    let watcher = Watcher::connect();
    let keep = TestWindow::open("Keep", &keep_id);
    let close = TestWindow::open("Close", &close_id);
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

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_window_its_client_unmaps_is_removed() {
    Compositor::current();
    let app_id = unique_app_id("unmapped");
    let watcher = Watcher::connect();
    let test_window = TestWindow::open("Brief", &app_id);
    watcher.wait_window("the window", &app_id, |_| true);
    drop(test_window);
    watcher.wait_gone("the unmapped window", &app_id);
}

//! Switching: through the crate, seen by the compositor; and by the
//! compositor's own means, seen through the crate.

use deskrinch_test_client::{Paint, TestClient};
use deskrinch_test_input::{Input, Key};
use deskrinch_test_screen::{Pictures, Rgba, Screen};
use deskrinch_workspaces::{Action, WorkspaceState, Workspaces};

use crate::support::{Compositor, Watcher, compositor, kwin, set_up};
use deskrinch_test_wait::eventually;

/// A colour nothing else in a session has.
const MARK: Rgba = Rgba::opaque(201, 17, 143);

/// Whether any output shows [`MARK`].
fn marked(screen: &mut Screen) -> bool {
    screen.outputs().iter().any(|output| {
        let image = screen.capture(output).unwrap();
        image.pixels().contains(&MARK)
    })
}

/// Waits until [`MARK`] is on screen (`shown`) or nowhere on it.
fn wait_marked(screen: &mut Screen, shown: bool) {
    let what = format!(
        "the window on the first workspace {} the screen",
        if shown { "on" } else { "off" }
    );
    eventually(&what, || marked(screen) == shown);
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_switch_through_the_crate_is_made_by_the_compositor() {
    let compositor = compositor();
    let watcher = Watcher::connect();
    set_up(compositor, &watcher);
    let target = watcher.inactive();
    watcher
        .manager
        .perform(target.id, Action::Activate)
        .unwrap();
    let active = watcher.wait_active(&target.name);
    assert_eq!(active.id, target.id);
    // The compositor's own word for it.
    match compositor {
        Compositor::Kwin => assert_eq!(
            kwin().reads().current_virtual_desktop().unwrap().name,
            target.name
        ),
        Compositor::Labwc => {
            // Another connection, told by the compositor.
            let other = Watcher::connect();
            other.wait_active(&target.name);
        }
        Compositor::Sway => panic!("sway runs only the unsupported tests"),
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_switch_away_hides_the_windows_left_behind() {
    let compositor = compositor();
    let watcher = Watcher::connect();
    set_up(compositor, &watcher);
    let mut screen = Screen::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let window = client
        .open_painted(
            "Left behind",
            "org.deskrinch.test.LeftBehind",
            Paint::Solid(MARK),
            None,
        )
        .unwrap();
    wait_marked(&mut screen, true);
    let target = watcher.inactive();
    watcher.activate(&target.name);
    // The switch hides it: the first capture without it shows the change.
    wait_marked(&mut screen, false);
    // And it stays hidden: on a still picture of each output, which a
    // return within the still gap would show...
    for output in screen.outputs() {
        Pictures::judged(
            &mut screen,
            &output,
            "the window left behind, off a still screen",
            |image| {
                (!image.pixels().contains(&MARK))
                    .then_some(())
                    .ok_or("the window left behind is on the screen")
            },
        );
    }
    // ...and in the compositor's own state, as the crate holds it after the
    // still picture: the workspace switched to is still the active one, so
    // the window's workspace is still the one left.
    let listed = watcher.listed();
    let active: Vec<_> = listed
        .iter()
        .filter(|workspace| workspace.state.contains(WorkspaceState::ACTIVE))
        .map(|workspace| workspace.name.as_str())
        .collect();
    assert_eq!(active, [target.name.as_str()], "{listed:#?}");
    drop(window);
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_switch_made_by_the_compositor_is_seen() {
    let compositor = compositor();
    let watcher = Watcher::connect();
    set_up(compositor, &watcher);
    let listed = watcher.listed();
    match compositor {
        Compositor::Kwin => {
            let target = watcher.inactive();
            kwin().set_current_virtual_desktop(&target.name).unwrap();
            watcher.wait_active(&target.name);
        }
        Compositor::Labwc => {
            // F12, the sessions' binding to the next workspace.
            let mut input = Input::connect().unwrap();
            input.press(Key::F12).unwrap();
            watcher.wait_active(&listed[1].name);
        }
        Compositor::Sway => panic!("sway runs only the unsupported tests"),
    }
    let after = watcher.listed();
    let active = after
        .iter()
        .filter(|workspace| workspace.state.contains(WorkspaceState::ACTIVE))
        .count();
    assert_eq!(active, 1, "{after:#?}");
}

//! An output that changes while the wallpaper runs keeps its window: moved,
//! rescaled or turned, and put back, the wallpaper has the windows it
//! started with, and every output is covered at its new shape. One test per
//! change, so that each shows on its own.
//!
//! Before the fix this guards, every such change closed the output's window
//! and opened a new one: the wallpaper's `for` over the outputs drew a row
//! again whenever its item changed, and its item was rinch's whole
//! `Output`, which changes with the output's position and scale.
//!
//! Which windows the wallpaper has is read from rinch's per-window lines:
//! a window that closes writes `closed`, a new one `opened`. Each step ends
//! on an ordered event, not a quiet window: after the compositor reports
//! the change to the test client, the test writes a new colour, and waits
//! for every window to present a frame since and every output to show it.
//! The compositor sent the change to the wallpaper before it answered the
//! test client, and the colour reaches the wallpaper only after its
//! settings watcher's pause, so the frame for the colour is drawn after the
//! wallpaper has read the change, and a window it re-made would be among
//! the windows by then.

use std::collections::BTreeSet;

use deskrinch_test_client::{OutputChange, OutputState, TestClient, change_output, outputs};
use deskrinch_test_component::{Mark, PerfWindow};
use deskrinch_test_screen::{Rgba, Screen};
use deskrinch_test_wait::wait_for;
use deskrinch_test_wayland::Transform;

use crate::support::{BLUE, Wallpaper, in_nested_session, wait_covered, write_settings, written};

/// The colour after the change, and after it is put back: unlike each
/// other and the colour the wallpaper starts with, none a compositor's own
/// background.
const CHANGED: Rgba = Rgba::opaque(0xd6, 0x24, 0x5c);
/// See [`CHANGED`].
const PUT_BACK: Rgba = Rgba::opaque(0x2a, 0xb0, 0x4e);

fn settings(color: Rgba) -> String {
    format!("[wallpaper]\ncolor = \"{}\"\n", written(color))
}

/// Every window the wallpaper has made so far, closed ones included.
fn ever(wallpaper: &Wallpaper) -> BTreeSet<PerfWindow> {
    wallpaper
        .watch()
        .since(&Mark::default())
        .iter()
        .map(|since| since.window)
        .collect()
}

/// The wallpaper's windows open now.
fn open(wallpaper: &Wallpaper) -> BTreeSet<PerfWindow> {
    wallpaper
        .watch()
        .windows()
        .iter()
        .map(|state| state.window)
        .collect()
}

/// The physical size of the output `name`, as captured.
fn captured_size(name: &str) -> (u32, u32) {
    let image = Screen::connect().unwrap().capture(name).unwrap();
    (image.width(), image.height())
}

/// Ends the step `what`: writes `color`, waits for every window to present a
/// frame since and every output to be all `color`, then checks that the
/// wallpaper has the windows it started with (`started`) and has made no
/// other.
fn step(wallpaper: &Wallpaper, what: &str, color: Rgba, started: &BTreeSet<PerfWindow>) {
    let mark = wallpaper.mark();
    write_settings(&settings(color));
    wallpaper.wait_each_window_drew_since(&mark);
    wait_covered(color);
    assert_eq!(
        open(wallpaper),
        *started,
        "after {what}, the windows open are not those the wallpaper started with; its last \
         lines:\n{}",
        wallpaper.log()
    );
    assert_eq!(
        ever(wallpaper),
        *started,
        "after {what}, the wallpaper had made another window; its last lines:\n{}",
        wallpaper.log()
    );
}

/// Starts the wallpaper, makes the change `make` returns for the second
/// output (given the layout), as `what`, and checks the windows after it
/// and after it is put back. `settled` waits, if it must, until the change
/// shows on the screen.
fn keeps_its_window(
    what: &str,
    make: impl FnOnce(&OutputState) -> OutputChange,
    settled: impl FnOnce(&str),
) {
    in_nested_session();
    write_settings(&settings(BLUE));
    let wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    let started = open(&wallpaper);
    assert_eq!(
        ever(&wallpaper),
        started,
        "a window closed before the test began"
    );
    let layout = outputs().unwrap();
    assert_eq!(layout.len(), 2, "two outputs: {layout:?}");
    let name = layout[1].name.clone();
    let change = make(&layout[0]);
    let changed = change_output(&name, change).unwrap_or_else(|error| panic!("{what}: {error}"));
    settled(&name);
    step(&wallpaper, what, CHANGED, &started);
    changed.restore().unwrap();
    step(&wallpaper, &format!("{what}, put back"), PUT_BACK, &started);
    wallpaper.assert_no_error();
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_output_moved_keeps_its_window() {
    keeps_its_window(
        "the move",
        |first| {
            // Below the first output, where neither was.
            let (_, height) = TestClient::connect()
                .unwrap()
                .output_info(&first.name)
                .and_then(|info| info.logical_size)
                .expect("the first output's logical size");
            OutputChange {
                position: Some((first.position.0, first.position.1 + height)),
                ..OutputChange::default()
            }
        },
        |_| {},
    );
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_output_rescaled_keeps_its_window() {
    in_nested_session();
    let scale = outputs().unwrap()[1].scale;
    keeps_its_window(
        "the new scale",
        // Twice its scale, or 1 from a scale above it.
        |_| OutputChange {
            scale: Some(if scale > 1.0 { 1.0 } else { 2.0 }),
            ..OutputChange::default()
        },
        |_| {},
    );
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_output_turned_keeps_its_window() {
    in_nested_session();
    let second = outputs().unwrap().remove(1);
    let (width, height) = captured_size(&second.name);
    // A quarter turn from what it is: its picture's sides swap.
    let to = if second.transform == Transform::Normal {
        Transform::Rotated90
    } else {
        Transform::Normal
    };
    keeps_its_window(
        "the turn",
        |_| OutputChange {
            transform: Some(to),
            ..OutputChange::default()
        },
        |name| {
            wait_for(&format!("{name} to turn"), || {
                let size = captured_size(name);
                if size == (height, width) {
                    Ok(())
                } else {
                    Err(size)
                }
            });
        },
    );
}

//! Outputs that change while the wallpaper runs: turned, and turned off
//! and on again, under every compositor. Each change is
//! `deskrinch-test-client`'s, whose guard puts the session's whole layout
//! back as it was, also when the test fails.

use std::collections::BTreeSet;

use deskrinch_test_client::{OutputChange, TestClient, change_output, turn_output_off};
use deskrinch_test_component::{Mark, PerfWindow, Watch};
use deskrinch_test_screen::{Rgba, Screen};
use deskrinch_test_wait::{eventually, wait_for};
use deskrinch_test_wayland::Transform;

use crate::support::{
    BLUE, Wallpaper, first_other, in_nested_session, wait_covered, wait_output, write_settings,
    written,
};

/// A colour unlike [`BLUE`] and no compositor's own background: the
/// settings edit that ends a step.
const STEP: Rgba = Rgba::opaque(0x2a, 0xb0, 0x4e);

/// The wallpaper's open window on the output `name`, as its `opened` line
/// says, if it has one.
fn window_on(watch: &Watch, name: &str) -> Option<PerfWindow> {
    watch
        .windows()
        .into_iter()
        .find(|state| {
            state
                .opened
                .as_ref()
                .is_some_and(|opened| opened.output.as_deref() == Some(name))
        })
        .map(|state| state.window)
}

/// Every window the wallpaper has made so far, closed ones included.
fn ever(watch: &Watch) -> BTreeSet<PerfWindow> {
    watch
        .since(&Mark::default())
        .iter()
        .map(|since| since.window)
        .collect()
}

/// The physical size of the output `name`, as captured.
fn captured_size(name: &str) -> (u32, u32) {
    let image = Screen::connect().unwrap().capture(name).unwrap();
    (image.width(), image.height())
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_output_turned_while_it_runs_is_covered_at_its_new_shape() {
    in_nested_session();
    write_settings(&format!("[wallpaper]\ncolor = \"{}\"\n", written(BLUE)));
    let _wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    let name = Screen::connect().unwrap().outputs().remove(0);
    let was = TestClient::connect()
        .unwrap()
        .output_info(&name)
        .unwrap()
        .transform;
    let (width, height) = captured_size(&name);
    // A quarter turn from what it is: its picture's sides swap.
    let to = if was == Transform::Normal {
        Transform::Rotated90
    } else {
        Transform::Normal
    };
    let turned = OutputChange {
        transform: Some(to),
        ..OutputChange::default()
    };
    let restore = change_output(&name, turned).unwrap();
    wait_for(&format!("{name} to turn"), || {
        let size = captured_size(&name);
        if size == (height, width) {
            Ok(())
        } else {
            Err(size)
        }
    });
    wait_covered(BLUE);
    restore.restore().unwrap();
}

/// Waits until the screen lists `name` (`listed`) or does not.
fn wait_listed(name: &str, listed: bool) {
    let is_listed = || {
        Screen::connect()
            .unwrap()
            .outputs()
            .iter()
            .any(|n| n == name)
    };
    let what = format!(
        "{name} {} the outputs",
        if listed { "back among" } else { "gone from" }
    );
    eventually(&what, || is_listed() == listed);
}

/// An output turned off takes its wallpaper with it, and turned on again
/// gets one anew: to the wallpaper this is an output unplugged and one
/// plugged in, a `wl_output` global removed and a new one added with a new
/// id (see the suite's doc). Its window closes with its row in rinch's
/// output ids, the other output keeps the window it had, and turned on
/// again it gets exactly one new window; the wallpaper says no error and
/// runs on.
///
/// Which windows the wallpaper has is read from rinch's per-window lines,
/// each wait ending on an ordered event: the off output's `closed` line;
/// then, back on, the new window's first frame; then a settings edit that
/// every window draws, after which no window can still be on its way.
#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_output_turned_off_and_on_loses_it_and_gets_it_again() {
    in_nested_session();
    write_settings(&format!("[wallpaper]\ncolor = \"{}\"\n", written(BLUE)));
    let mut wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    let mut outputs = Screen::connect().unwrap().outputs();
    let name = outputs.pop().expect("two outputs");
    let other = outputs.first().expect("two outputs").clone();
    let watch = wallpaper.watch().clone();
    let gone = window_on(&watch, &name).expect("a window on the output turned off");
    let kept = window_on(&watch, &other).expect("a window on the other output");
    let started = ever(&watch);

    let off = turn_output_off(&name).unwrap();
    wait_for(&format!("{name}'s window to close"), || {
        if watch.windows().iter().any(|state| state.window == gone) {
            Err(watch.tail())
        } else {
            Ok(())
        }
    });
    assert_eq!(
        window_on(&watch, &other),
        Some(kept),
        "{other} lost its window as {name} went; the wallpaper's last lines:\n{}",
        watch.tail()
    );
    assert_eq!(
        ever(&watch),
        started,
        "a window was made as {name} went; the wallpaper's last lines:\n{}",
        watch.tail()
    );
    wait_listed(&name, false);
    assert!(wallpaper.running(), "the wallpaper ended with an output");
    wait_output(&other, |image| {
        first_other(image, BLUE).map(|at| format!("not all wallpaper: {at:?}"))
    });

    off.restore().unwrap();
    wait_listed(&name, true);
    let back = wait_for(&format!("a window on {name}, back on, to draw"), || {
        let back = window_on(&watch, &name)
            .filter(|window| !started.contains(window))
            .filter(|window| {
                watch
                    .windows()
                    .iter()
                    .any(|state| state.window == *window && state.first_frame.is_some())
            });
        back.ok_or_else(|| watch.tail())
    });
    // Every window draws a settings edit: a window still on its way would
    // be among them by then.
    let mark = wallpaper.mark();
    write_settings(&format!("[wallpaper]\ncolor = \"{}\"\n", written(STEP)));
    wallpaper.wait_each_window_drew_since(&mark);
    wait_covered(STEP);
    let mut expected = started;
    expected.insert(back);
    assert_eq!(
        ever(&watch),
        expected,
        "{name}, back on, did not get exactly one new window; the wallpaper's last lines:\n{}",
        watch.tail()
    );
    assert_eq!(
        window_on(&watch, &other),
        Some(kept),
        "{other} lost its window as {name} came back; the wallpaper's last lines:\n{}",
        watch.tail()
    );
    assert!(
        wallpaper.running(),
        "the wallpaper ended as an output came back"
    );
    // A window whose output went is nothing to put right: no error.
    wallpaper.assert_no_error();
}

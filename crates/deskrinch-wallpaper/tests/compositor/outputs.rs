//! Outputs that change while the wallpaper runs: turned, and turned off
//! and on again, under every compositor. Each change is
//! `deskrinch-test-client`'s, whose guard puts the session's whole layout
//! back as it was, also when the test fails.

use deskrinch_test_client::{OutputChange, TestClient, change_output, turn_output_off};
use deskrinch_test_screen::Screen;
use deskrinch_test_wait::{eventually, wait_for};
use deskrinch_test_wayland::Transform;

use crate::support::{
    BLUE, Wallpaper, first_other, in_nested_session, wait_covered, wait_output, write_settings,
    written,
};

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
        scale: None,
        transform: Some(to),
    };
    let restore = change_output(&name, turned).unwrap();
    wait_for(&format!("{name} turned"), || {
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
/// gets one anew: its window closes with its row in rinch's outputs, and a
/// new one opens when the output comes back (the old one's layer surface,
/// closed by the compositor, cannot come back). The others stay covered.
#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_output_turned_off_and_on_loses_it_and_gets_it_again() {
    in_nested_session();
    write_settings(&format!("[wallpaper]\ncolor = \"{}\"\n", written(BLUE)));
    let mut wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    let mut outputs = Screen::connect().unwrap().outputs();
    let name = outputs.pop().expect("two outputs");
    let off = turn_output_off(&name).unwrap();
    wait_listed(&name, false);
    assert!(wallpaper.running(), "the wallpaper ended with an output");
    for other in &outputs {
        wait_output(other, |image| {
            first_other(image, BLUE).map(|at| format!("not all wallpaper: {at:?}"))
        });
    }
    off.restore().unwrap();
    wait_listed(&name, true);
    wait_output(&name, |image| {
        first_other(image, BLUE).map(|at| format!("back on, not all wallpaper: {at:?}"))
    });
    assert!(
        wallpaper.running(),
        "the wallpaper ended as an output came back"
    );
    // A window whose output went is nothing to put right: no error.
    wallpaper.assert_no_error();
    wait_covered(BLUE);
}

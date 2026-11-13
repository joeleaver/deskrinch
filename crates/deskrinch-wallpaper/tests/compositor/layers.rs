//! What is above the wallpaper: windows, and the panel.

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use deskrinch_test_client::{Paint, TestClient};
use deskrinch_test_component::{Component, Renderer};
use deskrinch_test_screen::Screen;

use crate::support::{
    BLUE, ORANGE, PATIENCE, Wallpaper, about, first_other, in_nested_session, wait_covered,
    wait_output, write_settings, written,
};

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_window_is_above_it() {
    in_nested_session();
    write_settings(&format!("[wallpaper]\ncolor = \"{}\"\n", written(BLUE)));
    let _wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    let outputs = Screen::connect().unwrap().outputs();
    let (covered, bare) = (&outputs[0], &outputs[1]);
    let client = TestClient::connect().unwrap();
    let _window = client
        .open_painted(
            "Above the wallpaper",
            "org.deskrinch.test.Above",
            Paint::Solid(ORANGE),
            Some(covered),
        )
        .unwrap();
    wait_output(covered, |image| {
        first_other(image, ORANGE).map(|at| format!("the window is not over all of it: {at:?}"))
    });
    wait_output(bare, |image| {
        first_other(image, BLUE).map(|at| format!("the wallpaper is not all of it: {at:?}"))
    });
}

/// The panel binary, built beside this crate's in the same run: `cargo
/// xtask compositor-tests` builds every suite's binaries in one Cargo run,
/// the panel's suite among them.
fn panel_binary() -> PathBuf {
    let panel =
        PathBuf::from(env!("CARGO_BIN_EXE_deskrinch-wallpaper")).with_file_name("deskrinch-panel");
    assert!(
        panel.is_file(),
        "{} is not built: run these through `cargo xtask compositor-tests`, which builds it",
        panel.display()
    );
    panel
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn the_panel_is_above_it() {
    in_nested_session();
    // The panel at its defaults: along the bottom, 40 high.
    write_settings(&format!("[wallpaper]\ncolor = \"{}\"\n", written(BLUE)));
    let _wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    let renderer = Renderer::of_run();
    let panel = Component::start(Command::new(panel_binary()), renderer).unwrap();
    if let Err(why) = panel.wait_presented_with(renderer) {
        panic!("the panel: {why}");
    }
    // On the output the compositor gave it, its bottom row is the panel's,
    // and the middle of the output still the wallpaper's.
    let deadline = Instant::now() + PATIENCE;
    loop {
        let mut screen = Screen::connect().unwrap();
        let shown = screen.outputs().into_iter().any(|name| {
            let image = screen.capture(&name).unwrap();
            let bottom = image.row(image.height() - 2).unwrap();
            let middle = image.row(image.height() / 2).unwrap();
            !bottom.iter().all(|&pixel| about(pixel, BLUE))
                && middle.iter().all(|&pixel| about(pixel, BLUE))
        });
        if shown {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "no output shows the panel over the wallpaper"
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

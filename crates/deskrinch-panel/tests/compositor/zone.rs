//! What the panel reserves: a maximised window stops at it.

use std::time::{Duration, Instant};

use deskrinch_test_client::{Paint, TestClient, TestWindow};
use deskrinch_test_screen::{Image, Rgba, Screen};

use crate::support::{
    Band, Edge, PATIENCE, Panel, Pictures, in_nested_session, panel_table, scale_of, wait_band,
    write_settings,
};

/// A colour neither the panel nor a compositor's background has.
pub(crate) const WINDOW: Rgba = Rgba::opaque(13, 200, 77);

/// A maximised window filled with [`WINDOW`], once it has drawn at the
/// size of `output`'s logical area less `reserved` logical rows.
pub(crate) fn maximised(client: &TestClient, output: &str, reserved: u32) -> TestWindow {
    let window = client
        .open_painted(
            "Maximised",
            "org.deskrinch.test.Maximised",
            Paint::Solid(WINDOW),
            None,
        )
        .unwrap();
    window.set_maximized(true).unwrap();
    wait_window_size(client, &window, output, reserved);
    window
}

/// Waits until `window` has drawn at `output`'s logical size less
/// `reserved` rows: its configured size is the output's less the panel's
/// zone.
pub(crate) fn wait_window_size(
    client: &TestClient,
    window: &TestWindow,
    output: &str,
    reserved: u32,
) {
    let (width, height) = client.output_info(output).unwrap().logical_size.unwrap();
    let wanted = (width, height - reserved.cast_signed());
    if let Err(error) = window.wait_drawn("the output's size less the panel's zone", |size| {
        size == wanted
    }) {
        panic!(
            "the maximised window is not {wanted:?}, {output}'s logical size less the zone: {error}"
        );
    }
}

/// What is wrong with `image` for a maximised [`WINDOW`] beside the panel at
/// `band`, if anything: every row outside the band is the window's colour,
/// and the band is `panel` (the panel's own rows, as captured without the
/// window). Rows within `scale` of the band's edge are left out, where the
/// compositor filters the window it enlarges.
fn wrong(image: &Image, panel: &Image, band: Band, scale: u32) -> Option<String> {
    let rows = band.rows_in(image.height());
    for y in 0..image.height() {
        let near = y.abs_diff(rows.start) <= scale || y.abs_diff(rows.end) <= scale;
        if near {
            continue;
        }
        let row = image.row(y).unwrap();
        if rows.contains(&y) {
            if row != panel.row(y).unwrap() {
                return Some(format!("row {y}, in the panel's band, is not the panel's"));
            }
        } else if let Some(x) = row.iter().position(|&pixel| pixel != WINDOW) {
            return Some(format!(
                "row {y}, outside the panel's band, is {} at {x}, not the window's {WINDOW}",
                row[x]
            ));
        }
    }
    None
}

/// Captures `output` until it shows a maximised [`WINDOW`] beside the panel
/// at `band`, whose own rows are `panel`.
pub(crate) fn wait_window_beside(screen: &mut Screen, output: &str, panel: &Image, band: Band) {
    let scale = scale_of(output);
    let deadline = Instant::now() + PATIENCE;
    loop {
        let image = screen.capture(output).unwrap();
        let Some(why) = wrong(&image, panel, band, scale) else {
            return;
        };
        assert!(Instant::now() < deadline, "on {output}: {why}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The picture of `output` in `pictures`.
pub(crate) fn picture<'a>(pictures: &'a Pictures, output: &str) -> &'a Image {
    &pictures
        .0
        .iter()
        .find(|(name, _)| name == output)
        .unwrap()
        .1
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_maximised_window_stops_at_the_panel() {
    in_nested_session();
    write_settings(&panel_table(Edge::Bottom, 40));
    let mut screen = Screen::connect().unwrap();
    let before = Pictures::take(&mut screen);
    let _panel = Panel::start();
    let scale = scale_of(&screen.outputs()[0]);
    let band = Band::of(Edge::Bottom, 40, scale);
    let (output, shown) = wait_band(&mut screen, &before, band);
    let client = TestClient::connect().unwrap();
    let _window = maximised(&client, &output, 40);
    wait_window_beside(&mut screen, &output, picture(&shown, &output), band);
}

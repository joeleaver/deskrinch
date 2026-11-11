//! Where the panel is: at its configured edge and height, on one output,
//! and nowhere else.

use deskrinch_test_screen::Screen;

use crate::support::{
    Band, Edge, Panel, Pictures, in_nested_session, no_settings, panel_table, session_scale,
    wait_band, write_settings,
};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn by_default_it_runs_along_the_bottom_40_high() {
    in_nested_session();
    no_settings();
    let mut screen = Screen::connect().unwrap();
    let scale = session_scale(&screen);
    let before = Pictures::take(&mut screen);
    let _panel = Panel::start();
    wait_band(&mut screen, &before, Band::of(Edge::Bottom, 40, scale));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn it_starts_at_its_configured_edge_and_height() {
    in_nested_session();
    write_settings(&panel_table(Edge::Top, 56));
    let mut screen = Screen::connect().unwrap();
    let scale = session_scale(&screen);
    let before = Pictures::take(&mut screen);
    let _panel = Panel::start();
    wait_band(&mut screen, &before, Band::of(Edge::Top, 56, scale));
    no_settings();
}

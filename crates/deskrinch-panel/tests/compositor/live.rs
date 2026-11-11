//! The panel following its settings while it runs: an edit moves and
//! resizes it, a rejected one leaves it where it was.

use deskrinch_test_client::TestClient;
use deskrinch_test_screen::Screen;

use crate::support::{
    Band, Edge, Panel, Pictures, in_nested_session, panel_table, scale_of, wait_band,
    write_settings,
};
use crate::zone::{maximised, picture, wait_window_beside, wait_window_size};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn an_edit_moves_and_resizes_the_running_panel() {
    in_nested_session();
    write_settings(&panel_table(Edge::Bottom, 40));
    let mut screen = Screen::connect().unwrap();
    let scale = scale_of(&screen.outputs()[0]);
    let before = Pictures::take(&mut screen);
    let _panel = Panel::start();
    wait_band(&mut screen, &before, Band::of(Edge::Bottom, 40, scale));
    write_settings(&panel_table(Edge::Top, 56));
    wait_band(&mut screen, &before, Band::of(Edge::Top, 56, scale));
    write_settings(&panel_table(Edge::Bottom, 24));
    wait_band(&mut screen, &before, Band::of(Edge::Bottom, 24, scale));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_maximised_window_follows_the_zone_as_it_changes() {
    in_nested_session();
    write_settings(&panel_table(Edge::Bottom, 40));
    let mut screen = Screen::connect().unwrap();
    let scale = scale_of(&screen.outputs()[0]);
    let before = Pictures::take(&mut screen);
    let _panel = Panel::start();
    let (output, _) = wait_band(&mut screen, &before, Band::of(Edge::Bottom, 40, scale));
    let client = TestClient::connect().unwrap();
    let window = maximised(&client, &output, 40);

    write_settings(&panel_table(Edge::Top, 56));
    let top = Band::of(Edge::Top, 56, scale);
    // The panel's own rows, from before the window: the window's colour
    // must not show through them.
    wait_window_size(&client, &window, &output, 56);
    drop(window);
    let (_, shown) = wait_band(&mut screen, &before, top);
    let _window = maximised(&client, &output, 56);
    wait_window_beside(&mut screen, &output, picture(&shown, &output), top);
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_rejected_value_leaves_it_where_it_was() {
    in_nested_session();
    let at = panel_table(Edge::Top, 48);
    write_settings(&at);
    let mut screen = Screen::connect().unwrap();
    let scale = scale_of(&screen.outputs()[0]);
    let before = Pictures::take(&mut screen);
    let panel = Panel::start();
    let band = Band::of(Edge::Top, 48, scale);
    wait_band(&mut screen, &before, band);

    write_settings("[panel]\nedge = \"top\"\nheight = 0\n");
    panel.wait_rejected("panel.height");
    wait_band(&mut screen, &before, band);
    write_settings("[panel]\nedge = \"left\"\nheight = 80\n");
    panel.wait_rejected("panel.edge");
    wait_band(&mut screen, &before, band);
    write_settings(&at);
}

//! The panel without any output: the last one turned off, the process
//! keeps running and says nothing at warn or error level; the outputs back,
//! every one gets a bar, with the taskbar and the clock as before. Run by
//! its own row, under the compositors that let a nested session turn off
//! its last output: sway and labwc. KWin keeps its last output on (asked to
//! turn it off, it does not, and `kscreen-doctor` reports the outputs
//! unchanged), so it is not covered there.

use deskrinch_test_client::{TestClient, outputs, turn_output_off};
use deskrinch_test_screen::Screen;
use deskrinch_test_wait::wait_for;

use crate::support::{
    Band, Edge, Panel, Pictures, in_nested_session, panel_table, scale_of, wait_band,
    write_settings,
};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_panel_outlives_its_last_output_and_gives_it_a_bar_again() {
    in_nested_session();
    write_settings(&panel_table(Edge::Bottom, 40));
    let mut screen = Screen::connect().unwrap();
    let before = Pictures::still(&mut screen);
    let mut panel = Panel::start();
    let scale = scale_of(&screen.outputs()[0]);
    wait_band(&mut screen, &before, Band::of(Edge::Bottom, 40, scale));
    let client = TestClient::connect().unwrap();
    let _window = client
        .open("Still here", "org.deskrinch.LastOutput")
        .unwrap();
    let names: Vec<String> = outputs()
        .unwrap()
        .into_iter()
        .map(|output| output.name)
        .collect();
    // Each turned off in turn, the last one last; put back in the reverse
    // order when the guards drop.
    let offs: Vec<_> = names
        .iter()
        .map(|name| turn_output_off(name).unwrap_or_else(|error| panic!("{name} off: {error}")))
        .collect();
    wait_for("every bar to close", || {
        if panel.bars().is_empty() {
            Ok(())
        } else {
            Err(panel.bars())
        }
    });
    assert!(
        panel.running(),
        "the panel ended without outputs: {}",
        panel.log()
    );
    for off in offs.into_iter().rev() {
        off.restore().unwrap();
    }
    wait_for("a bar on every output again", || {
        let on: Vec<String> = panel.bars().into_iter().map(|(_, output)| output).collect();
        if names.iter().all(|name| on.contains(name)) {
            Ok(())
        } else {
            Err(on)
        }
    });
    // A capture of an output that went has stopped: capture anew.
    let mut screen = Screen::connect().unwrap();
    wait_band(&mut screen, &before, Band::of(Edge::Bottom, 40, scale));
    assert!(panel.running(), "{}", panel.log());
    assert_eq!(panel.warnings_and_errors(), Vec::<String>::new());
}

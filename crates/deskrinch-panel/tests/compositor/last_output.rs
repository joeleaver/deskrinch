//! The panel without any output: the last one turned off, the process
//! keeps running and says nothing at warn or error level; the outputs back,
//! every one gets a bar, showing the open window's button (found by its
//! icon's colour, as [`crate::taskbar`] finds it) and the clock's face. Run by
//! its own row, under the compositors that let a nested session turn off
//! its last output: sway and labwc. KWin keeps its last output on (asked to
//! turn it off, it does not, and `kscreen-doctor` reports the outputs
//! unchanged), so it is not covered there.

use deskrinch_test_client::{TestClient, outputs, turn_output_off};
use deskrinch_test_screen::{Image, Screen};
use deskrinch_test_wait::wait_for;

use crate::support::{
    Band, Edge, Panel, Pictures, in_nested_session, panel_table, scale_of, wait_band,
    write_settings,
};
use crate::taskbar::{RED, icon_in, install, near};

/// Whether the clock's face is drawn in `band` of `image`: right of the
/// bar's middle, which an empty taskbar leaves the bar's own colour (its
/// padding at the left end), some pixel is not that colour.
fn clock_drawn(image: &Image, band: Band) -> bool {
    let rows = band.rows_in(image.height());
    let Some(bar) = image.pixel(1, u32::midpoint(rows.start, rows.end)) else {
        return false;
    };
    rows.into_iter().any(|y| {
        (image.width() / 2..image.width())
            .any(|x| image.pixel(x, y).is_some_and(|pixel| !near(pixel, bar)))
    })
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_panel_outlives_its_last_output_and_gives_it_a_bar_again() {
    in_nested_session();
    write_settings(&panel_table(Edge::Bottom, 40));
    let dirs = tempfile::tempdir().unwrap();
    install(dirs.path());
    let mut screen = Screen::connect().unwrap();
    let before = Pictures::still(&mut screen);
    let mut command = Panel::command();
    command.env("XDG_DATA_HOME", dirs.path());
    let mut panel = Panel::start_with(command);
    let scale = scale_of(&screen.outputs()[0]);
    let band = Band::of(Edge::Bottom, 40, scale);
    wait_band(&mut screen, &before, band);
    let client = TestClient::connect().unwrap();
    let _window = client.open("Still here", RED.id).unwrap();
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
    wait_band(&mut screen, &before, band);
    wait_for(
        "every bar to show the window's button and the clock",
        || {
            let lacking: Vec<String> = names
                .iter()
                .filter(|name| {
                    let image = screen.capture(name).unwrap();
                    icon_in(&image, band, scale, RED).is_none() || !clock_drawn(&image, band)
                })
                .cloned()
                .collect();
            if lacking.is_empty() {
                Ok(())
            } else {
                Err(lacking)
            }
        },
    );
    // Nothing said at warn or error for the outputs going and coming back:
    // bounded by the bars' `placed` lines for an edit made now.
    panel.bound(Edge::Top, 40);
    assert!(panel.running(), "{}", panel.log());
    assert_eq!(panel.watch().warnings_and_errors(), Vec::<String>::new());
}

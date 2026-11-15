//! The panel under a fullscreen window goes on working: it follows its
//! settings while the compositor does not show it, and is drawn as they
//! left it once uncovered.
//!
//! This is rinch issue 2669's case for the panel. With the GPU renderer, a
//! present on a surface the compositor has stopped showing waited for a
//! frame callback that never came, which blocked the program's main thread
//! until the surface was shown again (sway and labwc; KWin paces with
//! `wp_fifo_v1` and was never affected).
//!
//! What shows the main thread working is the line it writes each time it
//! applies a new edge and height ([`deskrinch_panel::placed`]). The
//! compositor's reaction to the panel's new exclusive zone would not do:
//! sway does not reconfigure a window hidden behind a fullscreen one when
//! a zone changes.
//!
//! The test is a sequence of steps, each confirmed before the next is
//! written, because which present blocks is not certain: Mesa waits for
//! the previous frame's callback, so the first present after the panel is
//! covered may go through and a later one block; and two edits written
//! close together merge in the settings watcher's pause and are handled as
//! one. Each step is a new height, which the main thread applies (and says
//! so) and then repaints at, a present on the covered surface. Four steps
//! leave room for the first presents to go through and still make several
//! that would block.

use deskrinch_test_client::{Paint, TestClient};
use deskrinch_test_screen::{Rgba, Screen};
use deskrinch_test_wait::wait_for;

use crate::support::{
    Band, Edge, Panel, Pictures, in_nested_session, panel_table, scale_of, wait_band,
    write_settings,
};

/// The fullscreen window's colour: not the panel's.
const FRONT: Rgba = Rgba::opaque(201, 37, 160);

/// The heights the covered panel is given in turn, from 40.
const HEIGHTS: [u32; 4] = [56, 48, 64, 44];

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_covered_panel_follows_its_settings_and_is_drawn_as_they_left_it() {
    in_nested_session();
    write_settings(&panel_table(Edge::Bottom, 40));
    let mut screen = Screen::connect().unwrap();
    let scale = scale_of(&screen.outputs()[0]);
    let before = Pictures::still(&mut screen);
    let panel = Panel::start();
    let (output, _) = wait_band(&mut screen, &before, Band::of(Edge::Bottom, 40, scale));
    let client = TestClient::connect().unwrap();
    let front = client
        .open_painted(
            "Fullscreen",
            "org.deskrinch.test.Fullscreen",
            Paint::Solid(FRONT),
            None,
        )
        .unwrap();
    front.set_fullscreen(true, Some(&output)).unwrap();
    front.wait_fullscreen().unwrap();
    // The panel's band is the fullscreen window's, but for the pixels within
    // `scale` of the output's edges, which a compositor may filter (labwc
    // blends its edge rows at scale 2).
    let band = Band::of(Edge::Bottom, 40, scale);
    wait_for(
        &format!("the fullscreen window over the panel on {output}"),
        || {
            let image = screen.capture(&output).unwrap();
            let rows = band.rows_in(image.height());
            let inner = scale..image.width() - scale;
            let covered = (rows.start..rows.end - scale)
                .all(|y| inner.clone().all(|x| image.pixel(x, y).unwrap() == FRONT));
            if covered {
                Ok(())
            } else {
                Err("the panel's band is not the fullscreen window's yet".to_owned())
            }
        },
    );

    for (step, height) in (1..).zip(HEIGHTS) {
        write_settings(&panel_table(Edge::Bottom, height));
        if let Err(why) = panel.wait_placed(Edge::Bottom, height) {
            panic!(
                "step {step} of {}: the covered panel did not move to {height} high: {why}",
                HEIGHTS.len()
            );
        }
    }

    drop(front);
    let last = HEIGHTS[HEIGHTS.len() - 1];
    wait_band(&mut screen, &before, Band::of(Edge::Bottom, last, scale));
}

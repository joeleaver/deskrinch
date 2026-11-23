//! Each bar's first frame is drawn at its output's scale: on a scaled
//! output there is no first frame at scale 1 followed by one at the
//! output's. A guard of a property every component relies on, which already
//! held before rinch 2652: KWin, sway and labwc tell a layer surface its
//! scale before its first configure (rinch's commit e54c72b7 says so of the
//! layer-surface case), and this test passed in every session on rinch
//! 78528484f as on ac657148a.
//!
//! What a frame was drawn at is read from rinch's own account of it: the
//! `surface_px` counter of each bar's first `[PERF] window N frame` line,
//! its buffer's area in pixels. Should rinch rename the counter, the test fails
//! saying the counter was not found, not with a wrong area.

use deskrinch_test_client::TestClient;
use deskrinch_test_component::{counter, first_frame};

use crate::support::{Panel, in_nested_session, no_settings, scale_of};

/// The panel's height by default, in logical pixels.
const HEIGHT: u64 = 40;

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn its_first_frame_is_drawn_at_its_outputs_scale() {
    in_nested_session();
    no_settings();
    let panel = Panel::start();
    let client = TestClient::connect().unwrap();
    let lines = panel.watch().lines();
    let bars = panel.bars();
    assert!(!bars.is_empty(), "no bar: {}", panel.log());
    for (window, output) in bars {
        let scale = u64::from(scale_of(&output));
        let (width, _) = client
            .output_info(&output)
            .unwrap()
            .logical_size
            .expect("the output's logical size");
        let width = u64::try_from(width).unwrap();
        let line = first_frame(&lines, window)
            .unwrap_or_else(|| panic!("the first frame line of {output}'s bar"));
        let area = counter(line, "surface_px")
            .unwrap_or_else(|| panic!("rinch's surface_px counter was not found in {line:?}"));
        assert_eq!(
            area,
            width * HEIGHT * scale * scale,
            "{output}'s bar's first frame is not {width}x{HEIGHT} at scale {scale}: {line}"
        );
    }
}

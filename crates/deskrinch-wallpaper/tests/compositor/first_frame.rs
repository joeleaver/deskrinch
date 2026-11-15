//! Each of the wallpaper's windows draws its first frame at its output's
//! scale: rinch starts a layer surface at the scale of the output it asks
//! for (rinch 2652), so on a scaled output there is no first frame at scale
//! 1 followed by one at the output's.
//!
//! What a frame was drawn at is read from rinch's own account of it: the
//! `surface_px` counter of each window's first `[PERF] window <id> frame`
//! line, its buffer's area in pixels. Should rinch rename the counter, the
//! test fails saying the counter was not found, not with a wrong area.

use deskrinch_test_client::TestClient;
use deskrinch_test_component::{counter, first_frame};
use deskrinch_test_screen::Screen;

use crate::support::{Wallpaper, in_nested_session};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn each_first_frame_is_drawn_at_its_outputs_scale() {
    in_nested_session();
    let outputs = Screen::connect().unwrap().outputs();
    let client = TestClient::connect().unwrap();
    // The session gives every output one size and scale: each window's
    // buffer covers one of them.
    let info = client.output_info(&outputs[0]).unwrap();
    let scale = u64::try_from(info.scale).unwrap();
    let (width, height) = info.logical_size.expect("the output's logical size");
    let area = u64::try_from(width * height).unwrap() * scale * scale;
    let wallpaper = Wallpaper::start();
    let lines = wallpaper.watch().lines();
    let windows = wallpaper.watch().windows();
    assert_eq!(
        windows.len(),
        outputs.len(),
        "a window for each output: {windows:?}"
    );
    for window in windows {
        let line = first_frame(&lines, window.window)
            .unwrap_or_else(|| panic!("no first frame line for {:?}", window.window));
        let drawn = counter(line, "surface_px")
            .unwrap_or_else(|| panic!("rinch's surface_px counter was not found in {line:?}"));
        assert_eq!(
            drawn, area,
            "{:?}'s first frame is not {width}x{height} at scale {scale}: {line}",
            window.window
        );
    }
}

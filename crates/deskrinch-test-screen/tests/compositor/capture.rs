//! Capturing outputs as they are.

use deskrinch_test_client::{Compositor, TestClient};
use deskrinch_test_screen::{CaptureError, Method, Screen};

use crate::support::{QUADRANTS, capture_showing, fullscreen};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_method_is_what_the_compositor_offers() {
    Compositor::current();
    let client = TestClient::connect().unwrap();
    let ext = client.offers("ext_image_copy_capture_manager_v1")
        && client.offers("ext_output_image_capture_source_manager_v1");
    let expected = if ext {
        Method::ExtImageCopyCapture
    } else if client.offers("zwlr_screencopy_manager_v1") {
        Method::WlrScreencopy
    } else {
        // KWin offers neither Wayland protocol.
        Method::KwinScreenShot2
    };
    let screen = Screen::connect().unwrap();
    assert_eq!(screen.method(), expected);
    let mut outputs = screen.outputs();
    outputs.sort();
    assert_eq!(outputs, client.output_names());
    assert_eq!(outputs.len(), 2, "{outputs:?}");
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_window_is_captured_pixel_for_pixel_on_its_own_output() {
    Compositor::current();
    let mut screen = Screen::connect().unwrap();
    let outputs = screen.outputs();
    let (shown, other) = (&outputs[1], &outputs[0]);
    let client = TestClient::connect().unwrap();
    let window = fullscreen(&client, shown, QUADRANTS);
    // Fullscreen at the output's logical size: its mode over its scale.
    let logical = (1280 / window.scale, 720 / window.scale);
    assert_eq!(window.size, logical);
    assert_eq!(
        window.window.drawn(),
        Some((logical.0.cast_signed(), logical.1.cast_signed()))
    );
    let image = capture_showing(&mut screen, shown, &window);
    // Physical pixels, whatever the scale: the output's mode.
    assert_eq!((image.width(), image.height()), (1280, 720));

    // The other output, captured by its own name, shows none of it.
    let elsewhere = screen.capture(other).unwrap();
    assert_eq!((elsewhere.width(), elsewhere.height()), (1280, 720));
    let painted = [(0, 0), (1279, 0), (0, 719), (1279, 719)]
        .map(|corner| QUADRANTS.color_at(corner, (1280, 720)));
    assert!(
        elsewhere
            .pixels()
            .iter()
            .all(|pixel| !painted.contains(pixel)),
        "the window shows on {other} too"
    );
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn an_output_that_is_not_there_is_named_in_the_error() {
    Compositor::current();
    let mut screen = Screen::connect().unwrap();
    let error = screen.capture("NOWHERE-9").unwrap_err();
    assert!(
        matches!(&error, CaptureError::UnknownOutput { name, known } if name == "NOWHERE-9" && known.len() == 2),
        "{error:?}"
    );
}

//! What only the wlroots compositors (sway, labwc) let the crate be tested
//! on: `wlr-screencopy`, which they offer beside the standard protocol, and
//! outputs scaled and rotated under a running client, through
//! `wlr-output-management`.
//!
//! KWin offers no `wlr-screencopy`, and its outputs are turned from the
//! start instead, in the `compositor` binary's rotated pass. `cargo xtask
//! compositor-tests` runs this binary under sway and labwc only, at scale 1
//! and at scale 2. Each change of an output is put back as the whole
//! layout was (`deskrinch_test_client::change_output`'s guard), also when
//! the test fails.

// `cfg(test)` marks them as test code, where clippy allows `unwrap`.
#[cfg(test)]
#[path = "../support/mod.rs"]
mod support;

#[cfg(test)]
mod tests {
    use deskrinch_test_client::{OutputChange, TestClient, change_output};
    use deskrinch_test_screen::{Method, Screen, Transform};

    use crate::support::{QUADRANTS, capture_showing, fullscreen};

    #[test]
    #[ignore = "needs a nested sway or labwc: cargo xtask compositor-tests"]
    fn wlr_screencopy_captures_the_same() {
        let mut screen = Screen::connect_with(Method::WlrScreencopy).unwrap();
        assert_eq!(screen.method(), Method::WlrScreencopy);
        let output = screen.outputs()[0].clone();
        let client = TestClient::connect().unwrap();
        let window = fullscreen(&client, &output, QUADRANTS);
        let image = capture_showing(&mut screen, &output, &window);
        assert_eq!((image.width(), image.height()), (1280, 720));
    }

    #[test]
    #[ignore = "needs a nested sway or labwc: cargo xtask compositor-tests"]
    fn a_scale_changed_under_a_running_client_is_captured_in_physical_pixels() {
        let mut screen = Screen::connect().unwrap();
        let output = screen.outputs()[0].clone();
        let client = TestClient::connect().unwrap();
        let mut window = fullscreen(&client, &output, QUADRANTS);
        capture_showing(&mut screen, &output, &window);

        // Another whole scale than the session's, either way.
        let other: u32 = if window.scale == 2 { 1 } else { 2 };
        let restore = change_output(
            &output,
            OutputChange {
                scale: Some(f64::from(other)),
                ..OutputChange::default()
            },
        )
        .unwrap();
        // The running window is configured to the new logical size, and
        // redraws at it.
        let logical = (1280 / other, 720 / other);
        let size = window
            .window
            .wait_drawn("the new logical size", |size| {
                size == (logical.0.cast_signed(), logical.1.cast_signed())
            })
            .unwrap();
        window.size = (size.0.cast_unsigned(), size.1.cast_unsigned());
        window.scale = client.output_scale(&output).unwrap().cast_unsigned();
        assert_eq!(window.scale, other);
        let image = capture_showing(&mut screen, &output, &window);
        assert_eq!((image.width(), image.height()), (1280, 720));
        restore.restore().unwrap();
    }

    #[test]
    #[ignore = "needs a nested sway or labwc: cargo xtask compositor-tests"]
    fn a_rotated_output_is_captured_upright() {
        let mut screen = Screen::connect().unwrap();
        let output = screen.outputs()[1].clone();
        let restore = change_output(
            &output,
            OutputChange {
                transform: Some(Transform::Rotated90),
                ..OutputChange::default()
            },
        )
        .unwrap();
        let client = TestClient::connect().unwrap();
        let window = fullscreen(&client, &output, QUADRANTS);
        // The window's top-left quarter is the image's top left, and the
        // image is the output's mode turned: 720 wide, 1280 high.
        let image = capture_showing(&mut screen, &output, &window);
        assert_eq!((image.width(), image.height()), (720, 1280));
        restore.restore().unwrap();
    }
}

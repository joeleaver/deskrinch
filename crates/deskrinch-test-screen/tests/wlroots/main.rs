//! What only the wlroots compositors (sway, labwc) let the crate be tested
//! on: `wlr-screencopy`, which they offer beside the standard protocol, and
//! outputs scaled and rotated under a running client, through
//! `wlr-output-management`.
//!
//! KWin offers neither, and its own output protocol cannot be used from
//! Rust here (see `deskrinch_test_client::configure_output`); its outputs
//! are turned from the start instead, in the `compositor` binary's rotated
//! pass. `cargo xtask compositor-tests` runs this binary under sway and
//! labwc only, at scale 1 and at scale 2.

// `cfg(test)` marks them as test code, where clippy allows `unwrap`.
#[cfg(test)]
#[path = "../support/mod.rs"]
mod support;

#[cfg(test)]
mod tests {
    use deskrinch_test_client::{OutputChange, OutputSettings, TestClient, configure_output};
    use deskrinch_test_screen::{Method, Screen, Transform};

    use crate::support::{QUADRANTS, capture_showing, fullscreen};

    /// Puts an output back as it was when dropped, so that a failing test
    /// leaves the next one the session it expects.
    struct Restore<'a> {
        output: &'a str,
        before: OutputSettings,
    }

    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            if let Err(error) = configure_output(self.output, self.before.into()) {
                eprintln!("could not put {} back: {error}", self.output);
            }
        }
    }

    /// Applies `change` to `output` until the returned guard is dropped.
    fn changed(output: &str, change: OutputChange) -> Restore<'_> {
        let before = configure_output(output, change).unwrap();
        Restore { output, before }
    }

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
        let _restore = changed(
            &output,
            OutputChange {
                scale: Some(f64::from(other)),
                ..OutputChange::default()
            },
        );
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
    }

    #[test]
    #[ignore = "needs a nested sway or labwc: cargo xtask compositor-tests"]
    fn a_rotated_output_is_captured_upright() {
        let mut screen = Screen::connect().unwrap();
        let output = screen.outputs()[1].clone();
        let _restore = changed(
            &output,
            OutputChange {
                transform: Some(Transform::Rotated90),
                ..OutputChange::default()
            },
        );
        let client = TestClient::connect().unwrap();
        let window = fullscreen(&client, &output, QUADRANTS);
        // The window's top-left quarter is the image's top left, and the
        // image is the output's mode turned: 720 wide, 1280 high.
        let image = capture_showing(&mut screen, &output, &window);
        assert_eq!((image.width(), image.height()), (720, 1280));
    }
}

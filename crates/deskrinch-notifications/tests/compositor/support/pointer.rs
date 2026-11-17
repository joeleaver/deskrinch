//! The pointer: moved and pressed at a point of the screen's pixels on the
//! column's output, and heard by windows of the test's own under the
//! column ([`Beneath`]), which say whether a press went through the
//! column's window to what is under it.

use deskrinch_test_client::{Paint, PointerEvent, TestClient, TestWindow};
use deskrinch_test_input::Button;
use deskrinch_test_screen::Rgba;

use super::{Column, Notifications, in_nested_session};

/// How far a point a window heard may be from the one the pointer was
/// sent to, in logical pixels: the compositor's fixed-point rounding.
const NEAR: f64 = 0.5;

impl Notifications {
    /// The logical point on `column`'s output at the middle of the screen's
    /// pixel `(x, y)`.
    pub(crate) fn logical(&self, x: u32, y: u32) -> (f64, f64) {
        let scale = f64::from(self.scale);
        ((f64::from(x) + 0.5) / scale, (f64::from(y) + 0.5) / scale)
    }

    /// Moves the pointer to `(x, y)`, the screen's pixels on `column`'s
    /// output.
    pub(crate) fn point(&mut self, column: &Column, x: u32, y: u32) {
        let (x, y) = self.logical(x, y);
        self.input.move_pointer(&column.output, x, y).unwrap();
    }

    /// Clicks the primary button where the pointer is.
    pub(crate) fn press(&mut self) {
        self.input.click(Button::PRIMARY).unwrap();
    }

    /// Clicks the primary button at `(x, y)`, as [`point`](Self::point).
    pub(crate) fn click(&mut self, column: &Column, x: u32, y: u32) {
        self.point(column, x, y);
        self.press();
    }

    /// Moves the pointer off the column, to the output's top-left corner.
    pub(crate) fn point_away(&mut self, column: &Column) {
        self.input.move_pointer(&column.output, 1.0, 1.0).unwrap();
    }
}

/// A window of the test's own on every output, fullscreen and painted, so
/// that whatever output the column is on, a window is under all of it and
/// hears the pointer the column lets through. The column's layer, the
/// overlay, is above fullscreen windows under every compositor.
///
/// Opened before [`Notifications::start`], so that the screen the column
/// is found against has them.
pub(crate) struct Beneath {
    /// By output name. Before the client, so that they go first.
    windows: Vec<(String, TestWindow)>,
    _client: TestClient,
}

impl Beneath {
    /// A window fullscreen on each output, once each is.
    pub(crate) fn open() -> Self {
        in_nested_session();
        let client = TestClient::connect().unwrap();
        let paint = Paint::Solid(Rgba::opaque(40, 90, 160));
        let windows = client
            .output_names()
            .into_iter()
            .map(|output| {
                let window = client
                    .open_painted(
                        &format!("beneath on {output}"),
                        "org.deskrinch.NotificationsTests.Beneath",
                        paint,
                        Some(&output),
                    )
                    .unwrap();
                window.wait_fullscreen().unwrap();
                (output, window)
            })
            .collect();
        Self {
            windows,
            _client: client,
        }
    }

    /// The window under `column`.
    pub(crate) fn under(&self, column: &Column) -> &TestWindow {
        self.windows
            .iter()
            .find_map(|(output, window)| (*output == column.output).then_some(window))
            .unwrap_or_else(|| panic!("no window on {}", column.output))
    }
}

/// Whether `events`, from `mark` on, have the pointer come over the window
/// at `at` (within [`NEAR`]) and not leave since.
pub(crate) fn over_since(events: &[PointerEvent], mark: usize, at: (f64, f64)) -> bool {
    let since = events.get(mark..).unwrap_or_default();
    since
        .iter()
        .rposition(|event| {
            event
                .point()
                .is_some_and(|(x, y)| (x - at.0).abs() <= NEAR && (y - at.1).abs() <= NEAR)
        })
        .is_some_and(|came| !since[came..].contains(&PointerEvent::Left))
}

/// Whether `events`, from `mark` on, have a whole click: a button pressed,
/// then released. A click is heard whole before the next is waited for, so
/// that a late release is not taken for the next click.
pub(crate) fn clicked_since(events: &[PointerEvent], mark: usize) -> bool {
    let since = events.get(mark..).unwrap_or_default();
    since
        .iter()
        .position(|event| matches!(event, PointerEvent::Pressed { .. }))
        .is_some_and(|at| {
            since[at..]
                .iter()
                .any(|event| matches!(event, PointerEvent::Released { .. }))
        })
}

/// Whether `events`, from `mark` on, have a button pressed or released.
pub(crate) fn pressed_since(events: &[PointerEvent], mark: usize) -> bool {
    events.get(mark..).unwrap_or_default().iter().any(|event| {
        matches!(
            event,
            PointerEvent::Pressed { .. } | PointerEvent::Released { .. }
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_since_wants_the_point_after_the_mark_and_no_leave_after_it() {
        let came = PointerEvent::Moved { x: 10.25, y: 20.0 };
        assert!(over_since(&[came], 0, (10.0, 20.0)));
        assert!(!over_since(&[came], 1, (10.0, 20.0)));
        assert!(!over_since(&[came, PointerEvent::Left], 0, (10.0, 20.0)));
        assert!(!over_since(&[came], 0, (12.0, 20.0)));
    }

    #[test]
    fn a_click_is_a_press_then_its_release_after_the_mark() {
        let press = PointerEvent::Pressed { button: 272 };
        let release = PointerEvent::Released { button: 272 };
        assert!(clicked_since(&[press, release], 0));
        assert!(!clicked_since(&[press], 0));
        assert!(!clicked_since(&[release, press], 0));
        assert!(!clicked_since(&[press, release], 1));
    }

    #[test]
    fn pressed_since_counts_from_the_mark() {
        let press = PointerEvent::Pressed { button: 272 };
        assert!(pressed_since(&[press], 0));
        assert!(!pressed_since(&[press], 1));
    }
}

//! Which outputs a window is on, from geometry: for protocols that report a
//! window's position rather than its outputs.
//!
//! Pure: rectangles in the compositor's global logical coordinate space (the
//! space `zxdg_output_v1` describes outputs in).

use std::collections::BTreeSet;

pub(crate) use deskrinch_wayland_client::Rect;

/// Whether the two share any area. Touching edges share none, and an empty
/// rectangle shares none with anything.
fn overlaps(this: Rect, other: Rect) -> bool {
    let span = |start: i32, length: i32| {
        (
            i64::from(start),
            i64::from(start) + i64::from(length.max(0)),
        )
    };
    let (left, right) = span(this.x, this.width);
    let (top, bottom) = span(this.y, this.height);
    let (other_left, other_right) = span(other.x, other.width);
    let (other_top, other_bottom) = span(other.y, other.height);
    left.max(other_left) < right.min(other_right) && top.max(other_top) < bottom.min(other_bottom)
}

/// The names of the outputs `window` overlaps.
pub(crate) fn outputs_under<'a>(
    window: Rect,
    outputs: impl IntoIterator<Item = (&'a str, Rect)>,
) -> BTreeSet<String> {
    outputs
        .into_iter()
        .filter(|&(_, output)| overlaps(window, output))
        .map(|(name, _)| name.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Two 1280×720 outputs side by side.
    const LAYOUT: [(&str, Rect); 2] = [
        ("Left", rect(0, 0, 1280, 720)),
        ("Right", rect(1280, 0, 1280, 720)),
    ];

    fn under(window: Rect) -> Vec<String> {
        outputs_under(window, LAYOUT).into_iter().collect()
    }

    #[test]
    fn a_window_inside_one_output_is_on_it() {
        assert_eq!(under(rect(100, 100, 400, 300)), ["Left"]);
        assert_eq!(under(rect(1300, 0, 1280, 720)), ["Right"]);
    }

    #[test]
    fn a_window_across_the_boundary_is_on_both() {
        assert_eq!(under(rect(1000, 100, 600, 300)), ["Left", "Right"]);
    }

    #[test]
    fn touching_an_edge_is_not_being_on_it() {
        assert_eq!(under(rect(1280 - 400, 0, 400, 720)), ["Left"]);
        assert_eq!(under(rect(0, 720, 100, 100)), Vec::<String>::new());
    }

    #[test]
    fn a_window_off_every_output_is_on_none() {
        assert_eq!(under(rect(-500, -500, 100, 100)), Vec::<String>::new());
    }

    #[test]
    fn an_empty_window_is_on_none() {
        assert_eq!(under(rect(100, 100, 0, 300)), Vec::<String>::new());
        assert_eq!(under(rect(100, 100, -5, 300)), Vec::<String>::new());
    }

    #[test]
    fn extreme_coordinates_do_not_overflow() {
        assert_eq!(
            under(rect(i32::MAX - 1, i32::MAX - 1, i32::MAX, i32::MAX)),
            Vec::<String>::new()
        );
        assert_eq!(
            under(rect(i32::MIN, 0, i32::MAX, 720)),
            Vec::<String>::new()
        );
    }
}

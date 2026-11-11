//! An output's transform, and undoing it.

/// How an output's picture is rotated and flipped on its way to the
/// physical display: Wayland's `wl_output.transform`. Rotations are
/// counter-clockwise; a flipped transform mirrors around the vertical axis,
/// then rotates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Transform {
    /// As is.
    #[default]
    Normal,
    /// A quarter turn.
    Rotated90,
    /// A half turn.
    Rotated180,
    /// Three quarter turns.
    Rotated270,
    /// Mirrored.
    Flipped,
    /// Mirrored, then a quarter turn.
    Flipped90,
    /// Mirrored, then a half turn.
    Flipped180,
    /// Mirrored, then three quarter turns.
    Flipped270,
}

impl Transform {
    /// Every transform, in `wl_output.transform`'s order.
    pub const ALL: [Self; 8] = [
        Self::Normal,
        Self::Rotated90,
        Self::Rotated180,
        Self::Rotated270,
        Self::Flipped,
        Self::Flipped90,
        Self::Flipped180,
        Self::Flipped270,
    ];

    /// The transform with `wl_output.transform` value `value`.
    #[must_use]
    pub fn from_wayland(value: u32) -> Option<Self> {
        Self::ALL.get(usize::try_from(value).ok()?).copied()
    }

    /// Its `wl_output.transform` value.
    #[must_use]
    pub fn to_wayland(self) -> u32 {
        match self {
            Self::Normal => 0,
            Self::Rotated90 => 1,
            Self::Rotated180 => 2,
            Self::Rotated270 => 3,
            Self::Flipped => 4,
            Self::Flipped90 => 5,
            Self::Flipped180 => 6,
            Self::Flipped270 => 7,
        }
    }

    /// Whether it swaps width and height.
    #[must_use]
    pub fn swaps_axes(self) -> bool {
        matches!(
            self,
            Self::Rotated90 | Self::Rotated270 | Self::Flipped90 | Self::Flipped270
        )
    }

    /// Where the upright picture's pixel `(x, y)` lies in a buffer that
    /// holds the picture with this transform applied, where `(width,
    /// height)` is the upright picture's size.
    #[must_use]
    pub fn buffer_position(self, (x, y): (u32, u32), (width, height): (u32, u32)) -> (u32, u32) {
        let right = width.saturating_sub(1).saturating_sub(x);
        let bottom = height.saturating_sub(1).saturating_sub(y);
        match self {
            Self::Normal => (x, y),
            Self::Rotated90 => (y, right),
            Self::Rotated180 => (right, bottom),
            Self::Rotated270 => (bottom, x),
            Self::Flipped => (right, y),
            Self::Flipped90 => (y, x),
            Self::Flipped180 => (x, bottom),
            Self::Flipped270 => (bottom, right),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The buffer a transform makes of a picture, by turning and mirroring
    /// it step by step: the definition `buffer_position` must agree with.
    fn apply(transform: Transform, picture: &[Vec<u32>]) -> Vec<Vec<u32>> {
        let mirror = |rows: &[Vec<u32>]| -> Vec<Vec<u32>> {
            rows.iter()
                .map(|row| row.iter().rev().copied().collect())
                .collect()
        };
        // A quarter turn counter-clockwise: the top row becomes the left
        // column, read from the bottom.
        let turn = |rows: &[Vec<u32>]| -> Vec<Vec<u32>> {
            let width = rows[0].len();
            (0..width)
                .rev()
                .map(|column| rows.iter().map(|row| row[column]).collect())
                .collect()
        };
        let (flipped, turns) = match transform {
            Transform::Normal => (false, 0),
            Transform::Rotated90 => (false, 1),
            Transform::Rotated180 => (false, 2),
            Transform::Rotated270 => (false, 3),
            Transform::Flipped => (true, 0),
            Transform::Flipped90 => (true, 1),
            Transform::Flipped180 => (true, 2),
            Transform::Flipped270 => (true, 3),
        };
        let mut rows = if flipped {
            mirror(picture)
        } else {
            picture.to_vec()
        };
        for _ in 0..turns {
            rows = turn(&rows);
        }
        rows
    }

    #[test]
    fn buffer_positions_follow_the_definition() {
        let (width, height) = (4_u32, 3_u32);
        let picture: Vec<Vec<u32>> = (0..height)
            .map(|y| (0..width).map(|x| y * width + x).collect())
            .collect();
        for transform in Transform::ALL {
            let buffer = apply(transform, &picture);
            let expected = if transform.swaps_axes() {
                (height, width)
            } else {
                (width, height)
            };
            assert_eq!(
                (buffer[0].len(), buffer.len()),
                (expected.0 as usize, expected.1 as usize),
                "{transform:?}"
            );
            for y in 0..height {
                for x in 0..width {
                    let (bx, by) = transform.buffer_position((x, y), (width, height));
                    assert_eq!(
                        buffer[by as usize][bx as usize], picture[y as usize][x as usize],
                        "{transform:?} at ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn wayland_values_round_trip() {
        for transform in Transform::ALL {
            assert_eq!(
                Transform::from_wayland(transform.to_wayland()),
                Some(transform)
            );
        }
        assert_eq!(Transform::from_wayland(8), None);
    }
}

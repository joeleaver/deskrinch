//! Where a point on a named output is in the compositor's layout, and how
//! wlroots is given it: the crate documentation's "Where a point goes".

use deskrinch_test_client::TestClient;

use crate::Error;

/// An output's place in the layout, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

/// Every output's place, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Layout {
    outputs: Vec<(String, Place)>,
}

impl Layout {
    /// The layout as `client` knows it now.
    ///
    /// # Errors
    ///
    /// [`Error::Unplaced`] for an output whose position or size has not been
    /// reported.
    pub(crate) fn of(client: &TestClient) -> Result<Self, Error> {
        let outputs = client
            .output_names()
            .into_iter()
            .map(|name| {
                let info = client.output_info(&name);
                let position = info.and_then(|info| info.logical_position);
                let size = info.and_then(|info| info.logical_size);
                match (position, size) {
                    (Some((x, y)), Some((width, height))) => Ok((
                        name,
                        Place {
                            x,
                            y,
                            width,
                            height,
                        },
                    )),
                    _ => Err(Error::Unplaced(name)),
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { outputs })
    }

    /// The global point `(x, y)` on `output` is.
    ///
    /// # Errors
    ///
    /// [`Error::NoOutput`] for an output the layout does not have, and
    /// [`Error::OffOutput`] for a point outside it.
    pub(crate) fn global(&self, output: &str, x: f64, y: f64) -> Result<(f64, f64), Error> {
        let (_, place) = self
            .outputs
            .iter()
            .find(|(name, _)| name == output)
            .ok_or_else(|| Error::NoOutput(output.to_owned()))?;
        let inside = (0.0..f64::from(place.width)).contains(&x)
            && (0.0..f64::from(place.height)).contains(&y);
        if !inside {
            return Err(Error::OffOutput {
                output: output.to_owned(),
                x,
                y,
                size: (place.width, place.height),
            });
        }
        Ok((f64::from(place.x) + x, f64::from(place.y) + y))
    }

    /// The box around every output: what wlroots maps absolute motion onto.
    pub(crate) fn bounds(&self) -> Place {
        let left = self.outputs.iter().map(|(_, p)| p.x).min().unwrap_or(0);
        let top = self.outputs.iter().map(|(_, p)| p.y).min().unwrap_or(0);
        let right = self
            .outputs
            .iter()
            .map(|(_, p)| p.x + p.width)
            .max()
            .unwrap_or(0);
        let bottom = self
            .outputs
            .iter()
            .map(|(_, p)| p.y + p.height)
            .max()
            .unwrap_or(0);
        Place {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }
    }
}

/// How finely wlroots' extent divides a logical pixel, so that a point
/// between pixels is not rounded to one.
pub(crate) const SUBPIXELS: f64 = 256.0;

/// The global point `(x, y)` as wlroots' `motion_absolute` takes it: a
/// position and an extent on each axis, against the layout's `bounds`.
pub(crate) fn against_extent(bounds: Place, (x, y): (f64, f64)) -> (u32, u32, u32, u32) {
    let along = |offset: f64, origin: i32| (offset - f64::from(origin)) * SUBPIXELS;
    let extent = |length: i32| f64::from(length) * SUBPIXELS;
    (
        whole(along(x, bounds.x)),
        whole(along(y, bounds.y)),
        whole(extent(bounds.width)),
        whole(extent(bounds.height)),
    )
}

/// `value`, rounded, as the protocol's unsigned integer; never negative,
/// since every point is inside the bounds.
fn whole(value: f64) -> u32 {
    // Bounded by the layout's size times SUBPIXELS, far inside u32.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a non-negative position within the layout, times 256"
    )]
    let whole = value.round().max(0.0) as u32;
    whole
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two 1280×720 outputs side by side, the second at x = 1280, as the
    /// compositor tests' sessions have them.
    fn side_by_side() -> Layout {
        let place = |x| Place {
            x,
            y: 0,
            width: 1280,
            height: 720,
        };
        Layout {
            outputs: vec![("A".into(), place(0)), ("B".into(), place(1280))],
        }
    }

    #[test]
    fn a_point_on_the_second_output_is_offset_by_its_position() {
        let layout = side_by_side();
        assert_eq!(layout.global("A", 10.0, 20.0).unwrap(), (10.0, 20.0));
        assert_eq!(layout.global("B", 10.0, 20.0).unwrap(), (1290.0, 20.0));
    }

    #[test]
    fn a_point_off_the_output_or_an_unknown_output_is_refused() {
        let layout = side_by_side();
        assert!(matches!(
            layout.global("A", 1280.0, 0.0),
            Err(Error::OffOutput {
                size: (1280, 720),
                ..
            })
        ));
        assert!(matches!(
            layout.global("A", -1.0, 0.0),
            Err(Error::OffOutput { .. })
        ));
        assert!(matches!(layout.global("C", 0.0, 0.0), Err(Error::NoOutput(name)) if name == "C"));
    }

    #[test]
    fn the_bounds_take_in_every_output_wherever_it_is() {
        let mut layout = side_by_side();
        assert_eq!(
            layout.bounds(),
            Place {
                x: 0,
                y: 0,
                width: 2560,
                height: 720
            }
        );
        layout.outputs[0].1.x = -640;
        layout.outputs[0].1.y = 100;
        assert_eq!(
            layout.bounds(),
            Place {
                x: -640,
                y: 0,
                width: 3200,
                height: 820
            }
        );
    }

    #[test]
    fn wlroots_gets_the_point_against_the_bounds_in_subpixels() {
        let layout = side_by_side();
        let bounds = layout.bounds();
        let point = layout.global("B", 10.5, 20.0).unwrap();
        assert_eq!(
            against_extent(bounds, point),
            (1290 * 256 + 128, 20 * 256, 2560 * 256, 720 * 256)
        );
    }
}

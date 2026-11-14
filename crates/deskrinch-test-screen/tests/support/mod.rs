//! What the compositor tests share: the picture they put on screen, and
//! comparing a capture with it.

use std::time::Duration;

use deskrinch_test_client::{Paint, TestClient, TestWindow};
use deskrinch_test_screen::{CaptureError, Image, Rgba, Screen};
use deskrinch_test_wait::PATIENCE;

/// Four colours no compositor draws by itself: a picture whose orientation,
/// size and position the capture must all get right.
pub(crate) const QUADRANTS: Paint = Paint::Quadrants {
    top_left: Rgba::opaque(200, 30, 40),
    top_right: Rgba::opaque(30, 190, 60),
    bottom_left: Rgba::opaque(40, 50, 210),
    bottom_right: Rgba::opaque(230, 210, 20),
};

/// A window on screen, and what an output must show for it.
#[derive(Debug)]
pub(crate) struct Shown {
    /// The window; dropping it closes it.
    pub(crate) window: TestWindow,
    /// What it draws.
    pub(crate) paint: Paint,
    /// The size it drew, in logical pixels.
    pub(crate) size: (u32, u32),
    /// The scale of the output it is on, as the output reports it.
    pub(crate) scale: u32,
}

/// A window painted `paint`, fullscreen on `output`, once it has drawn at
/// the size the compositor gave it (the output's logical size).
pub(crate) fn fullscreen(client: &TestClient, output: &str, paint: Paint) -> Shown {
    let window = client
        .open_painted(
            "Captured",
            "org.deskrinch.test.Captured",
            paint,
            Some(output),
        )
        .unwrap();
    let (width, height) = window.wait_fullscreen().unwrap();
    let scale = client.output_scale(output).unwrap();
    Shown {
        window,
        paint,
        size: (width.try_into().unwrap(), height.try_into().unwrap()),
        scale: scale.try_into().unwrap(),
    }
}

/// How far a channel may be from the colour drawn: compositors may round
/// when they composite (labwc 0.7 under pixman gives 0xc7 for a window's
/// 0xc8 in some regions).
const ROUNDING: u8 = 2;

/// Whether `got` is `want` within [`ROUNDING`] in each channel.
fn close(got: Rgba, want: Rgba) -> bool {
    [
        (got.r, want.r),
        (got.g, want.g),
        (got.b, want.b),
        (got.a, want.a),
    ]
    .into_iter()
    .all(|(got, want)| got.abs_diff(want) <= ROUNDING)
}

/// Whether `image` is `paint` drawn at `size` and shown at `scale`
/// physical pixels per logical one; `Err` says where it is not.
///
/// At a scale above 1 the compositor filters as it enlarges, so the pixels
/// within one logical pixel of a seam between two colours, or of the
/// window's edge, are left out. Each channel may be off by
/// [`ROUNDING`].
pub(crate) fn shows(
    image: &Image,
    paint: Paint,
    size: (u32, u32),
    scale: u32,
) -> Result<(), String> {
    let expected = (size.0 * scale, size.1 * scale);
    if (image.width(), image.height()) != expected {
        return Err(format!(
            "the image is {}x{}, not {}x{}",
            image.width(),
            image.height(),
            expected.0,
            expected.1
        ));
    }
    let seams = (size.0 / 2 * scale, size.1 / 2 * scale);
    let near = |at: u32, line: u32| scale > 1 && at.abs_diff(line) <= scale;
    let near_seam = |at: u32, seam: u32, end: u32| near(at, seam) || near(at, 0) || near(at, end);
    let mut wrong = 0_u64;
    let mut first = None;
    for y in 0..image.height() {
        for x in 0..image.width() {
            if near_seam(x, seams.0, expected.0 - 1) || near_seam(y, seams.1, expected.1 - 1) {
                continue;
            }
            let want = paint.color_at((x / scale, y / scale), size);
            let got = image.pixel(x, y).unwrap();
            if !close(got, want) {
                wrong += 1;
                first.get_or_insert((x, y, got, want));
            }
        }
    }
    match first {
        None => Ok(()),
        Some((x, y, got, want)) => Err(format!(
            "{wrong} pixels differ; the first at ({x}, {y}) is {got}, not {want}"
        )),
    }
}

/// Captures `output` until it shows `shown`.
pub(crate) fn capture_showing(screen: &mut Screen, output: &str, shown: &Shown) -> Image {
    let Shown {
        paint, size, scale, ..
    } = *shown;
    match screen.capture_until(output, PATIENCE, |image| {
        shows(image, paint, size, scale).is_ok()
    }) {
        Ok(image) => image,
        Err(CaptureError::NotSeen { last, .. }) => panic!(
            "{output} never showed the window: {}",
            shows(&last, paint, size, scale).unwrap_err()
        ),
        Err(error) => panic!("capturing {output}: {error}"),
    }
}

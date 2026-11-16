//! GIF: the first frame, placed on its screen.

use deskrinch_picture::Format;
use deskrinch_test_pictures::gif::{Placed, encode, with_screen};

use crate::{decoded, pixel};

fn flat(left: u16, top: u16, width: u16, height: u16, colour: [u8; 4]) -> Placed {
    Placed {
        left,
        top,
        width,
        height,
        rgba: colour.repeat(usize::from(width) * usize::from(height)),
    }
}

#[test]
fn a_gif_gives_its_first_frame() {
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let bytes = encode(4, 3, &[flat(0, 0, 4, 3, red), flat(0, 0, 4, 3, blue)]);
    let decoded = decoded(&bytes).unwrap();
    assert_eq!(decoded.format, Format::Gif);
    assert_eq!((decoded.image.width(), decoded.image.height()), (4, 3));
    assert_eq!(pixel(&decoded, 3, 2), red);
}

#[test]
fn a_frame_sits_where_it_says_on_a_transparent_screen() {
    let green = [0, 255, 0, 255];
    let bytes = encode(8, 6, &[flat(2, 1, 3, 2, green)]);
    let decoded = decoded(&bytes).unwrap();
    assert_eq!((decoded.image.width(), decoded.image.height()), (8, 6));
    assert_eq!(pixel(&decoded, 2, 1), green);
    assert_eq!(pixel(&decoded, 4, 2), green);
    assert_eq!(pixel(&decoded, 1, 1)[3], 0);
    assert_eq!(pixel(&decoded, 5, 2)[3], 0);
    assert_eq!(pixel(&decoded, 2, 3)[3], 0);
}

#[test]
fn a_frame_past_the_screen_is_clipped_and_a_screen_of_nothing_takes_the_frame_s_size() {
    let green = [0, 255, 0, 255];
    let past = with_screen(&encode(8, 6, &[flat(6, 4, 4, 4, green)]), 8, 6);
    let decoded = decoded(&past).unwrap();
    assert_eq!((decoded.image.width(), decoded.image.height()), (8, 6));
    assert_eq!(pixel(&decoded, 7, 5), green);

    let nothing = with_screen(&encode(4, 4, &[flat(0, 0, 4, 4, green)]), 0, 0);
    let decoded = crate::decoded(&nothing).unwrap();
    assert_eq!((decoded.image.width(), decoded.image.height()), (4, 4));
}

//! The worker's state as windows and outputs come and go: the plan is made
//! over each listed output's last surface, so windows closing while their
//! outputs are listed (as when the wallpaper ends) change nothing, and an
//! output unlisted is planned without once.

use super::*;

/// Output 1 at 1920x1080 and output 2 at 3840x2160: a 3840x2160 picture
/// covered is the picture itself on output 2, so on both.
const BOTH: [(u64, Surface); 2] = [
    (
        1,
        Surface {
            device: (1920, 1080),
            logical: (1920, 1080),
        },
    ),
    (
        2,
        Surface {
            device: (3840, 2160),
            logical: (3840, 2160),
        },
    ),
];

#[test]
fn windows_closing_while_their_outputs_are_listed_make_and_hand_over_nothing() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    let shown = state
        .step([listed(PICTURE, Fit::Cover, &[1, 2], &BOTH)])
        .unwrap();
    assert_eq!(names(&shown)[&2], "wallpaper/1/picture");
    // The larger window's row goes first, then the other's, the outputs
    // still listed: as when the wallpaper ends.
    assert_eq!(
        state.step([listed(PICTURE, Fit::Cover, &[1, 2], &BOTH[..1])]),
        None
    );
    assert_eq!(
        state.step([listed(PICTURE, Fit::Cover, &[1, 2], &[])]),
        None
    );
    assert_eq!(*decodes.borrow(), 1);
}

/// The larger output unplugged, its surface gone before it is unlisted or
/// after: the same end, once. The smaller output gets its made-smaller
/// image and the picture is let go.
#[test]
fn the_larger_output_unlisted_gives_the_smaller_its_own_image_in_either_order() {
    for surface_first in [true, false] {
        let (file, decodes) = files();
        let mut state = state(&file, &decodes);
        state.step([listed(PICTURE, Fit::Cover, &[1, 2], &BOTH)]);
        let between = if surface_first {
            listed(PICTURE, Fit::Cover, &[1, 2], &BOTH[..1])
        } else {
            listed(PICTURE, Fit::Cover, &[1], &BOTH)
        };
        assert_eq!(
            state.step([between]),
            None,
            "surface first: {surface_first}"
        );
        let shown = state
            .step([listed(PICTURE, Fit::Cover, &[1], &BOTH[..1])])
            .unwrap();
        assert_eq!(
            names(&shown),
            BTreeMap::from([(1, "wallpaper/1/cover/1920x1080".to_owned())]),
            "surface first: {surface_first}"
        );
        assert!(!shown.pictures.contains_key("wallpaper/1/picture"));
        assert!(!state.holds_picture());
    }
}

#[test]
fn a_window_made_again_at_its_size_after_the_picture_was_let_go_needs_no_decode() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    let both = [(1, surface(1280, 720)), (2, surface(1600, 900))];
    state.step([listed(PICTURE, Fit::Cover, &[1, 2], &both)]);
    assert!(!state.holds_picture());
    assert_eq!(
        state.step([listed(PICTURE, Fit::Cover, &[1, 2], &both[..1])]),
        None
    );
    assert_eq!(
        state.step([listed(PICTURE, Fit::Cover, &[1, 2], &both)]),
        None
    );
    assert_eq!(*decodes.borrow(), 1);
}

#[test]
fn a_window_made_again_at_another_size_is_decoded_for_once() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    let both = [(1, surface(1280, 720)), (2, surface(1600, 900))];
    state.step([listed(PICTURE, Fit::Cover, &[1, 2], &both)]);
    state.step([listed(PICTURE, Fit::Cover, &[1, 2], &both[..1])]);
    let again = state
        .step([listed(
            PICTURE,
            Fit::Cover,
            &[1, 2],
            &[both[0], (2, surface(1024, 768))],
        )])
        .unwrap();
    assert_eq!(names(&again)[&2], "wallpaper/1/cover/1024x768");
    assert_eq!(names(&again)[&1], "wallpaper/1/cover/1280x720");
    assert_eq!(*decodes.borrow(), 2);
    assert!(!state.holds_picture());
}

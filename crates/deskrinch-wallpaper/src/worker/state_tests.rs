//! The worker's state through a decoder of made-up pictures: what is
//! handed to the UI, what is shared, what is held, and what each failure
//! shows.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use deskrinch_untrusted::file::FileError;

use super::*;

const PICTURE: &str = "/pictures/a.png";

/// What the made-up file holds now: a picture of a size, damaged, or gone.
#[derive(Debug, Clone, Copy)]
enum File {
    Picture(u32, u32),
    Damaged,
    Gone,
}

/// A state whose decoder reads `file`, counting its decodes, and which
/// makes each image as the worker does.
fn state(
    file: &Rc<RefCell<File>>,
    decodes: &Rc<RefCell<u32>>,
) -> State<impl Decode + use<>, impl Make + use<>> {
    State::new(decoder(file, decodes), crate::worker::make)
}

/// A decoder that reads `file`, counting its decodes.
fn decoder(file: &Rc<RefCell<File>>, decodes: &Rc<RefCell<u32>>) -> impl Decode + use<> {
    let (file, decodes) = (Rc::clone(file), Rc::clone(decodes));
    move |path: &Path| {
        assert_eq!(path, Path::new(PICTURE));
        *decodes.borrow_mut() += 1;
        decoded(*file.borrow())
    }
}

/// What decoding a file holding `file` gives.
fn decoded(file: File) -> Result<Image, DecodeError> {
    match file {
        File::Picture(w, h) => Ok(Image::filled(w, h, |rgba| rgba.fill(u8::MAX)).unwrap()),
        File::Damaged => Err(DecodeError::NotRecognised),
        File::Gone => Err(DecodeError::File(FileError::Io(std::io::Error::from(
            ErrorKind::NotFound,
        )))),
    }
}

fn surface(w: u32, h: u32) -> Surface {
    Surface {
        device: (w, h),
        logical: (w, h),
    }
}

/// The picture wanted with `fit` on `surfaces`, each output listed with
/// its size.
fn wanted(fit: Fit, surfaces: &[(u64, Surface)]) -> Input {
    let ids: Vec<u64> = surfaces.iter().map(|&(id, _)| id).collect();
    listed(PICTURE, fit, &ids, surfaces)
}

/// The picture at `path` wanted with `fit`, the outputs `outputs` listed
/// and those of `surfaces` with a size.
fn listed(path: &str, fit: Fit, outputs: &[u64], surfaces: &[(u64, Surface)]) -> Input {
    Input::Wanted(Wanted {
        path: Some(PathBuf::from(path)),
        fit,
        outputs: outputs.iter().map(|&id| OutputId::from_raw(id)).collect(),
        surfaces: surfaces
            .iter()
            .map(|&(id, s)| (OutputId::from_raw(id), s))
            .collect(),
    })
}

fn names(shown: &Shown) -> BTreeMap<u64, String> {
    shown
        .names
        .iter()
        .map(|(id, name)| (id.raw(), name.clone()))
        .collect()
}

fn files() -> (Rc<RefCell<File>>, Rc<RefCell<u32>>) {
    (
        Rc::new(RefCell::new(File::Picture(3840, 2160))),
        Rc::new(RefCell::new(0)),
    )
}

#[test]
fn two_outputs_of_one_size_share_one_image_and_the_picture_is_let_go() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    let shown = state
        .step([wanted(
            Fit::Cover,
            &[(1, surface(1280, 720)), (2, surface(1280, 720))],
        )])
        .unwrap();
    assert_eq!(
        names(&shown),
        BTreeMap::from([
            (1, "wallpaper/1/cover/1280x720".to_owned()),
            (2, "wallpaper/1/cover/1280x720".to_owned()),
        ])
    );
    assert_eq!(shown.pictures.len(), 1);
    let image = &shown.pictures["wallpaper/1/cover/1280x720"];
    assert_eq!((image.width(), image.height()), (1280, 720));
    assert!(
        !state.holds_picture(),
        "made smaller: the picture is let go"
    );
    assert_eq!(*decodes.borrow(), 1);
}

#[test]
fn a_picture_shown_itself_is_kept_shared_and_made_smaller_ones_from_it_need_no_decode() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    let shown = state
        .step([wanted(Fit::Cover, &[(1, surface(3840, 2160))])])
        .unwrap();
    assert!(shown.pictures.contains_key("wallpaper/1/picture"));
    assert!(state.holds_picture(), "shown itself: rinch holds it anyway");
    // An equal want changes nothing.
    assert_eq!(
        state.step([wanted(Fit::Cover, &[(1, surface(3840, 2160))])]),
        None
    );
    // The output made smaller: made from the picture held, not decoded.
    let smaller = state
        .step([wanted(Fit::Cover, &[(1, surface(1920, 1080))])])
        .unwrap();
    assert_eq!(names(&smaller)[&1], "wallpaper/1/cover/1920x1080");
    assert_eq!(*decodes.borrow(), 1);
    assert!(!state.holds_picture());
    assert!(!smaller.pictures.contains_key("wallpaper/1/picture"));
}

#[test]
fn an_output_added_later_has_the_picture_decoded_again_and_one_removed_lets_go_of_its_image() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([wanted(Fit::Cover, &[(1, surface(1280, 720))])]);
    let added = state
        .step([wanted(
            Fit::Cover,
            &[(1, surface(1280, 720)), (2, surface(1600, 900))],
        )])
        .unwrap();
    assert_eq!(
        *decodes.borrow(),
        2,
        "the picture was let go: decoded again"
    );
    assert_eq!(
        added.pictures.keys().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "wallpaper/1/cover/1280x720".to_owned(),
            "wallpaper/1/cover/1600x900".to_owned()
        ]),
        "the same generation: the first output's image is kept as it was"
    );
    let removed = state
        .step([wanted(Fit::Cover, &[(1, surface(1280, 720))])])
        .unwrap();
    assert_eq!(removed.pictures.len(), 1);
    assert_eq!(names(&removed).len(), 1);
}

#[test]
fn a_changed_file_is_a_new_generation_for_every_output() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([wanted(Fit::Cover, &[(1, surface(1280, 720))])]);
    *file.borrow_mut() = File::Picture(2560, 1440);
    let shown = state.step([Input::Changed]).unwrap();
    assert_eq!(names(&shown)[&1], "wallpaper/2/cover/1280x720");
    assert_eq!(shown.pictures.len(), 1);
}

#[test]
fn a_fit_changed_hands_over_the_new_fits_image() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([wanted(Fit::Cover, &[(1, surface(1000, 1000))])]);
    let contain = state
        .step([wanted(Fit::Contain, &[(1, surface(1000, 1000))])])
        .unwrap();
    assert_eq!(names(&contain)[&1], "wallpaper/1/scaled/1000x562");
}

#[test]
fn a_first_load_that_fails_shows_the_colour_and_says_so() {
    let (file, decodes) = files();
    *file.borrow_mut() = File::Damaged;
    let mut state = state(&file, &decodes);
    let capture = deskrinch_test_log::capture();
    assert_eq!(
        state.step([wanted(Fit::Cover, &[(1, surface(1280, 720))])]),
        None,
        "nothing was shown, and nothing is"
    );
    assert_eq!(
        capture.lines(),
        [format!(
            "wallpaper: cannot show {PICTURE}: not a format this decodes (PNG, JPEG, GIF, WebP); \
             showing the colour alone"
        )]
    );
}

#[test]
fn a_replaced_file_that_fails_keeps_the_last_good_picture_and_says_so() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([wanted(Fit::Cover, &[(1, surface(1280, 720))])]);
    *file.borrow_mut() = File::Damaged;
    let capture = deskrinch_test_log::capture();
    assert_eq!(state.step([Input::Changed]), None, "what was shown stays");
    assert_eq!(
        capture.lines(),
        [format!(
            "wallpaper: cannot show {PICTURE} as it is now: not a format this decodes (PNG, \
             JPEG, GIF, WebP); keeping the picture shown before"
        )]
    );
    // Whole again: shown, a new generation.
    *file.borrow_mut() = File::Picture(3840, 2160);
    let shown = state.step([Input::Changed]).unwrap();
    assert_eq!(names(&shown)[&1], "wallpaper/2/cover/1280x720");
}

#[test]
fn a_file_taken_away_shows_the_colour() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([wanted(Fit::Cover, &[(1, surface(1280, 720))])]);
    *file.borrow_mut() = File::Gone;
    let capture = deskrinch_test_log::capture();
    assert_eq!(state.step([Input::Changed]), Some(Shown::default()));
    assert_eq!(
        capture.lines(),
        [format!(
            "wallpaper: {PICTURE} is gone; showing the colour alone"
        )]
    );
    assert!(!state.holds_picture());
}

#[test]
fn no_picture_named_is_the_colour_and_holds_nothing() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([wanted(Fit::Cover, &[(1, surface(3840, 2160))])]);
    let none = state
        .step([Input::Wanted(Wanted {
            path: None,
            ..Wanted::default()
        })])
        .unwrap();
    assert_eq!(none, Shown::default());
    assert!(!state.holds_picture());
}

#[test]
fn a_burst_is_one_step_and_one_decode() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    let shown = state
        .step([
            wanted(Fit::Cover, &[(1, surface(800, 600))]),
            Input::Changed,
            wanted(
                Fit::Cover,
                &[(1, surface(1280, 720)), (2, surface(1280, 720))],
            ),
            Input::Changed,
        ])
        .unwrap();
    assert_eq!(*decodes.borrow(), 1);
    assert_eq!(shown.pictures.len(), 1);
    assert_eq!(names(&shown).len(), 2);
}

#[test]
fn a_file_missing_at_the_first_load_cannot_be_shown_rather_than_gone() {
    let (file, decodes) = files();
    *file.borrow_mut() = File::Gone;
    let mut state = state(&file, &decodes);
    let capture = deskrinch_test_log::capture();
    assert_eq!(
        state.step([wanted(Fit::Cover, &[(1, surface(1280, 720))])]),
        None
    );
    let lines = capture.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with(&format!("wallpaper: cannot show {PICTURE}: the file: ")),
        "{lines:?}"
    );
    assert!(
        lines[0].ends_with("; showing the colour alone"),
        "{lines:?}"
    );
}

#[path = "state_start_tests.rs"]
mod start;

#[path = "state_outputs_tests.rs"]
mod outputs;

#[path = "state_kept_tests.rs"]
mod kept;

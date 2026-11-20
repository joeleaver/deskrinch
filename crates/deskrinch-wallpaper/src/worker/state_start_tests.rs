//! The worker's state at start and around a failed read: the picture is
//! decoded as soon as an output is listed, held until every listed output
//! has its pixels, and a file that cannot be shown is decoded and said
//! once until it or the path changes.

use super::*;

const OTHER: &str = "/pictures/b.png";

/// A state whose decoder reads `PICTURE` as `file` and `OTHER` as
/// `other`, writing down each path it decodes.
fn two_files(
    file: &Rc<RefCell<File>>,
    other: &Rc<RefCell<File>>,
    paths: &Rc<RefCell<Vec<String>>>,
) -> State<impl Decode + use<>, impl Make + use<>> {
    let (file, other, paths) = (Rc::clone(file), Rc::clone(other), Rc::clone(paths));
    let decode = move |path: &Path| {
        let path = path.to_str().unwrap().to_owned();
        let now = if path == PICTURE {
            *file.borrow()
        } else {
            *other.borrow()
        };
        paths.borrow_mut().push(path);
        decoded(now)
    };
    State::new(decode, crate::worker::make)
}

#[test]
fn an_output_listed_without_a_size_has_the_picture_decoded_once_and_nothing_said_or_shown() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    let capture = deskrinch_test_log::capture();
    assert_eq!(state.step([listed(PICTURE, Fit::Cover, &[1], &[])]), None);
    assert_eq!(*decodes.borrow(), 1, "decoded while the window opens");
    assert!(
        state.holds_picture(),
        "held for the output still to be served"
    );
    assert_eq!(state.step([listed(PICTURE, Fit::Cover, &[1], &[])]), None);
    assert_eq!(*decodes.borrow(), 1);
    assert!(capture.lines().is_empty(), "{:?}", capture.lines());
}

#[test]
fn a_size_after_the_decode_is_fitted_with_no_second_decode_and_the_picture_let_go() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([listed(PICTURE, Fit::Cover, &[1], &[])]);
    let shown = state
        .step([listed(
            PICTURE,
            Fit::Cover,
            &[1],
            &[(1, surface(1280, 720))],
        )])
        .unwrap();
    assert_eq!(names(&shown)[&1], "wallpaper/1/cover/1280x720");
    assert_eq!(*decodes.borrow(), 1);
    assert!(!state.holds_picture(), "every output served: let go");
}

#[test]
fn no_output_listed_decodes_nothing_and_one_listed_later_is_decoded_then() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    assert_eq!(state.step([listed(PICTURE, Fit::Cover, &[], &[])]), None);
    assert_eq!(*decodes.borrow(), 0);
    assert!(!state.holds_picture());
    state.step([listed(PICTURE, Fit::Cover, &[1], &[])]);
    assert_eq!(*decodes.borrow(), 1);
    assert!(state.holds_picture());
}

#[test]
fn the_outputs_gone_before_any_size_let_the_picture_go() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([listed(PICTURE, Fit::Cover, &[1, 2], &[])]);
    assert!(state.holds_picture());
    assert_eq!(state.step([listed(PICTURE, Fit::Cover, &[], &[])]), None);
    assert!(!state.holds_picture());
}

#[test]
fn a_picture_changed_during_the_early_decode_is_read_afresh_once_and_the_stale_one_never_shown() {
    let (file, decoded) = (files().0, Rc::new(RefCell::new(Vec::new())));
    let other = Rc::new(RefCell::new(File::Picture(2000, 2000)));
    let mut state = two_files(&file, &other, &decoded);
    state.step([listed(PICTURE, Fit::Contain, &[1], &[])]);
    state.step([listed(OTHER, Fit::Contain, &[1], &[])]);
    let shown = state
        .step([listed(
            OTHER,
            Fit::Contain,
            &[1],
            &[(1, surface(1280, 720))],
        )])
        .unwrap();
    assert_eq!(*decoded.borrow(), [PICTURE, OTHER]);
    // The other picture's square, not the first's 16:9.
    assert_eq!(names(&shown)[&1], "wallpaper/2/scaled/720x720");
    assert_eq!(shown.pictures.len(), 1);
}

#[test]
fn a_fit_changed_during_the_early_decode_needs_no_second_decode() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([listed(PICTURE, Fit::Cover, &[1], &[])]);
    state.step([listed(PICTURE, Fit::Contain, &[1], &[])]);
    let shown = state
        .step([listed(
            PICTURE,
            Fit::Contain,
            &[1],
            &[(1, surface(1000, 1000))],
        )])
        .unwrap();
    assert_eq!(names(&shown)[&1], "wallpaper/1/scaled/1000x562");
    assert_eq!(*decodes.borrow(), 1);
}

/// The fifth principle with the picture decoded before any need was known:
/// what is handed over is the made-smaller images, together less than the
/// picture, and the picture is held only while an output waits for its.
#[test]
fn what_is_held_in_all_is_never_more_than_the_picture_after_an_early_decode() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([listed(PICTURE, Fit::Cover, &[1, 2], &[])]);
    let first = state
        .step([listed(
            PICTURE,
            Fit::Cover,
            &[1, 2],
            &[(1, surface(1280, 720))],
        )])
        .unwrap();
    assert_eq!(names(&first).len(), 1);
    assert!(state.holds_picture(), "output 2 still waits for its pixels");
    let both = state
        .step([listed(
            PICTURE,
            Fit::Cover,
            &[1, 2],
            &[(1, surface(1280, 720)), (2, surface(1600, 900))],
        )])
        .unwrap();
    assert_eq!(*decodes.borrow(), 1);
    assert!(!state.holds_picture());
    let handed: u64 = both
        .pictures
        .values()
        .map(|image| u64::from(image.width()) * u64::from(image.height()) * 4)
        .sum();
    assert!(handed < 3840 * 2160 * 4, "{handed}");
    assert!(!both.pictures.contains_key("wallpaper/1/picture"));
}

#[test]
fn a_damaged_file_is_decoded_and_said_once_however_many_outputs_come() {
    let (file, decodes) = files();
    *file.borrow_mut() = File::Damaged;
    let mut state = state(&file, &decodes);
    let capture = deskrinch_test_log::capture();
    state.step([listed(PICTURE, Fit::Cover, &[1], &[])]);
    state.step([listed(
        PICTURE,
        Fit::Cover,
        &[1],
        &[(1, surface(1280, 720))],
    )]);
    state.step([wanted(
        Fit::Cover,
        &[(1, surface(1280, 720)), (2, surface(800, 600))],
    )]);
    assert_eq!(*decodes.borrow(), 1);
    assert_eq!(capture.lines().len(), 1, "{:?}", capture.lines());
}

#[test]
fn a_good_file_after_a_failed_load_is_shown_once_with_no_second_warning() {
    let (file, decodes) = files();
    *file.borrow_mut() = File::Damaged;
    let mut state = state(&file, &decodes);
    let capture = deskrinch_test_log::capture();
    state.step([wanted(Fit::Cover, &[(1, surface(1280, 720))])]);
    *file.borrow_mut() = File::Picture(3840, 2160);
    let shown = state.step([Input::Changed]).unwrap();
    assert_eq!(names(&shown)[&1], "wallpaper/1/cover/1280x720");
    assert_eq!(*decodes.borrow(), 2);
    assert_eq!(capture.lines().len(), 1, "{:?}", capture.lines());
}

#[test]
fn another_bad_path_is_decoded_and_said_once_and_the_first_never_again() {
    let (file, decoded) = (files().0, Rc::new(RefCell::new(Vec::new())));
    *file.borrow_mut() = File::Damaged;
    let other = Rc::new(RefCell::new(File::Damaged));
    let mut state = two_files(&file, &other, &decoded);
    let capture = deskrinch_test_log::capture();
    state.step([listed(
        PICTURE,
        Fit::Cover,
        &[1],
        &[(1, surface(1280, 720))],
    )]);
    state.step([listed(OTHER, Fit::Cover, &[1], &[(1, surface(1280, 720))])]);
    state.step([listed(
        OTHER,
        Fit::Cover,
        &[1, 2],
        &[(1, surface(1280, 720)), (2, surface(800, 600))],
    )]);
    assert_eq!(*decoded.borrow(), [PICTURE, OTHER]);
    let lines = capture.lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].contains(PICTURE), "{lines:?}");
    assert!(lines[1].contains(OTHER), "{lines:?}");
}

/// Every window closes when the wallpaper ends, its surface going while its
/// output is still listed: that output was served, and nothing is decoded
/// for it again.
#[test]
fn a_surface_gone_while_its_output_is_still_listed_is_not_decoded_for_again() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    let both = [(1, surface(1280, 720)), (2, surface(1280, 720))];
    state.step([listed(PICTURE, Fit::Cover, &[1, 2], &[])]);
    state.step([listed(PICTURE, Fit::Cover, &[1, 2], &both)]);
    assert!(!state.holds_picture());
    state.step([listed(PICTURE, Fit::Cover, &[1, 2], &both[1..])]);
    state.step([listed(PICTURE, Fit::Cover, &[1, 2], &[])]);
    assert_eq!(*decodes.borrow(), 1);
    assert!(!state.holds_picture());
}

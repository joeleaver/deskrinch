//! The worker's state while a picture is kept over a file gone bad: the
//! file is decoded and said once until it changes, an output keeps the
//! image it shows, and an output new to the picture shows the colour.

use super::*;

/// Outputs 1 and 2, each shown at a made size (so the picture was let go),
/// then the file damaged and changed: kept, with one decode and one line.
fn kept() -> (
    Rc<RefCell<File>>,
    Rc<RefCell<u32>>,
    State<impl Decode, impl Make>,
) {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([wanted(
        Fit::Cover,
        &[(1, surface(1280, 720)), (2, surface(1600, 900))],
    )]);
    assert!(!state.holds_picture());
    *file.borrow_mut() = File::Damaged;
    assert_eq!(state.step([Input::Changed]), None, "kept");
    assert_eq!(*decodes.borrow(), 2);
    (file, decodes, state)
}

#[test]
fn an_output_listed_then_sized_while_the_picture_is_kept_decodes_the_bad_file_no_more() {
    let capture = deskrinch_test_log::capture();
    let (_file, decodes, mut state) = kept();
    let both = [(1, surface(1280, 720)), (2, surface(1600, 900))];
    // Output 3 listed without a size, then sized, then resized.
    assert_eq!(
        state.step([listed(PICTURE, Fit::Cover, &[1, 2, 3], &both)]),
        None
    );
    let sized = [both[0], both[1], (3, surface(800, 600))];
    let shown = state.step([listed(PICTURE, Fit::Cover, &[1, 2, 3], &sized)]);
    assert_eq!(
        shown, None,
        "output 3 shows the colour; 1 and 2 keep theirs"
    );
    let resized = [both[0], both[1], (3, surface(1024, 768))];
    assert_eq!(
        state.step([listed(PICTURE, Fit::Cover, &[1, 2, 3], &resized)]),
        None
    );
    assert_eq!(*decodes.borrow(), 2, "the bad file was decoded once");
    let lines = capture.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("keeping the picture shown before"),
        "{lines:?}"
    );
}

#[test]
fn a_fit_changed_while_the_picture_is_kept_keeps_what_each_output_shows() {
    let capture = deskrinch_test_log::capture();
    let (_file, decodes, mut state) = kept();
    let contain = state.step([wanted(
        Fit::Contain,
        &[(1, surface(1280, 720)), (2, surface(1600, 900))],
    )]);
    assert_eq!(contain, None, "every output keeps the image it shows");
    assert_eq!(*decodes.borrow(), 2);
    assert_eq!(capture.lines().len(), 1, "{:?}", capture.lines());
}

#[test]
fn a_good_file_after_the_kept_state_is_decoded_once_and_shown_on_every_output() {
    let (file, decodes, mut state) = kept();
    let three = [
        (1, surface(1280, 720)),
        (2, surface(1600, 900)),
        (3, surface(800, 600)),
    ];
    state.step([wanted(Fit::Cover, &three)]);
    *file.borrow_mut() = File::Picture(3840, 2160);
    let shown = state.step([Input::Changed]).unwrap();
    assert_eq!(*decodes.borrow(), 3);
    assert_eq!(
        names(&shown),
        BTreeMap::from([
            (1, "wallpaper/2/cover/1280x720".to_owned()),
            (2, "wallpaper/2/cover/1600x900".to_owned()),
            (3, "wallpaper/2/cover/800x600".to_owned()),
        ])
    );
    // The kept state is gone: a new size is made as ever, decoding again.
    let resized = [three[0], three[1], (3, surface(1024, 768))];
    let again = state.step([wanted(Fit::Cover, &resized)]).unwrap();
    assert_eq!(names(&again)[&3], "wallpaper/2/cover/1024x768");
    assert_eq!(*decodes.borrow(), 4);
}

/// A file taken away whose change is still on its way, with two new needs
/// in one step: one failed decode, said once, and each output that shows
/// an image keeps it.
#[test]
fn two_new_needs_in_one_step_over_a_file_gone_are_one_failed_decode() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    state.step([wanted(Fit::Cover, &[(1, surface(1280, 720))])]);
    *file.borrow_mut() = File::Gone;
    let capture = deskrinch_test_log::capture();
    let shown = state.step([wanted(
        Fit::Cover,
        &[
            (1, surface(1280, 720)),
            (2, surface(1600, 900)),
            (3, surface(800, 600)),
        ],
    )]);
    assert_eq!(
        shown, None,
        "output 1 keeps its image; 2 and 3 show the colour"
    );
    assert_eq!(*decodes.borrow(), 2);
    assert_eq!(capture.lines().len(), 1, "{:?}", capture.lines());
}

/// A state whose decoder reads `file`, counting `decodes`, and whose maker
/// refuses [`REFUSED`], counting its `attempts` at it.
fn refusing(
    file: &Rc<RefCell<File>>,
    decodes: &Rc<RefCell<u32>>,
    attempts: &Rc<RefCell<u32>>,
) -> State<impl Decode + use<>, impl Make + use<>> {
    let counted = Rc::clone(attempts);
    let make = move |picture: &Image, need: Need| {
        if need == REFUSED {
            *counted.borrow_mut() += 1;
            Err(FitError::Resample("refused for the test".to_owned()))
        } else {
            crate::worker::make(picture, need)
        }
    };
    State::new(decoder(file, decodes), make)
}

const REFUSED: Need = Need::Cover {
    width: 1600,
    height: 900,
};

#[test]
fn a_refused_image_is_said_once_and_asked_for_no_more_with_no_decode() {
    let capture = deskrinch_test_log::capture();
    let ((file, decodes), attempts) = (files(), Rc::new(RefCell::new(0)));
    let mut state = refusing(&file, &decodes, &attempts);
    let both = [(1, surface(1280, 720)), (2, surface(1600, 900))];
    let shown = state.step([wanted(Fit::Cover, &both)]).unwrap();
    assert_eq!(names(&shown).len(), 1, "output 2 shows the colour");
    assert_eq!(names(&shown)[&1], "wallpaper/1/cover/1280x720");
    // The same wants again, and another output: the refused need is not
    // made again, and nothing is decoded for it.
    assert_eq!(state.step([wanted(Fit::Cover, &both)]), None);
    let three = [both[0], both[1], (3, surface(800, 600))];
    let more = state.step([wanted(Fit::Cover, &three)]).unwrap();
    assert_eq!(names(&more).len(), 2);
    assert_eq!(*attempts.borrow(), 1);
    assert_eq!(
        *decodes.borrow(),
        2,
        "one for output 3's image, none for 2's"
    );
    assert_eq!(capture.lines().len(), 1, "{:?}", capture.lines());
}

#[test]
fn an_output_whose_new_image_is_refused_keeps_the_one_it_shows() {
    let ((file, decodes), attempts) = (files(), Rc::new(RefCell::new(0)));
    let mut state = refusing(&file, &decodes, &attempts);
    let before = [(1, surface(1280, 720)), (2, surface(1920, 1080))];
    state.step([wanted(Fit::Cover, &before)]);
    let resized = [before[0], (2, surface(1600, 900))];
    assert_eq!(
        state.step([wanted(Fit::Cover, &resized)]),
        None,
        "output 2 keeps its 1920x1080 image"
    );
}

#[test]
fn a_refused_image_is_asked_for_again_for_a_new_picture() {
    let ((file, decodes), attempts) = (files(), Rc::new(RefCell::new(0)));
    let mut state = refusing(&file, &decodes, &attempts);
    let both = [(1, surface(1280, 720)), (2, surface(1600, 900))];
    state.step([wanted(Fit::Cover, &both)]);
    *file.borrow_mut() = File::Picture(2560, 1440);
    state.step([Input::Changed]);
    assert_eq!(*attempts.borrow(), 2);
}

/// The file gone bad and output 2 unlisted, in one step: the plan is made
/// all the same, so output 2's name and image go with it.
#[test]
fn an_output_unlisted_in_the_step_its_file_goes_bad_is_let_go() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    let three = [
        (1, surface(1280, 720)),
        (2, surface(1600, 900)),
        (3, surface(800, 600)),
    ];
    state.step([wanted(Fit::Cover, &three)]);
    *file.borrow_mut() = File::Damaged;
    let capture = deskrinch_test_log::capture();
    let shown = state
        .step([Input::Changed, wanted(Fit::Cover, &[three[0], three[2]])])
        .unwrap();
    assert_eq!(
        names(&shown),
        BTreeMap::from([
            (1, "wallpaper/1/cover/1280x720".to_owned()),
            (3, "wallpaper/1/cover/800x600".to_owned()),
        ])
    );
    assert!(!shown.pictures.contains_key("wallpaper/1/cover/1600x900"));
    assert_eq!(*decodes.borrow(), 2, "one failed decode");
    assert_eq!(capture.lines().len(), 1, "{:?}", capture.lines());
}

/// The picture itself shown (a 3840x2160 output beside a 1920x1080 one),
/// then in one step the file gone bad and output 3 sized: by the fifth
/// principle output 3 takes the picture too, served by the name already
/// shown, with no decode.
#[test]
fn an_output_sized_in_the_step_its_file_goes_bad_is_served_from_the_picture_shown() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    let two = [(1, surface(1920, 1080)), (2, surface(3840, 2160))];
    state.step([wanted(Fit::Cover, &two)]);
    assert!(state.holds_picture());
    *file.borrow_mut() = File::Damaged;
    let shown = state
        .step([
            Input::Changed,
            wanted(Fit::Cover, &[two[0], two[1], (3, surface(1280, 720))]),
        ])
        .unwrap();
    assert_eq!(names(&shown)[&3], "wallpaper/1/picture");
    assert_eq!(*decodes.borrow(), 2, "one failed decode");
}

/// Output 1 shown, output 2 listed without a size (so the picture is held
/// for it), then in one step the file gone bad and output 2 sized: its
/// image is made from the held picture, with no decode.
#[test]
fn an_output_sized_in_the_step_its_file_goes_bad_is_made_from_the_picture_held() {
    let (file, decodes) = files();
    let mut state = state(&file, &decodes);
    let one = [(1, surface(1280, 720))];
    state.step([listed(PICTURE, Fit::Cover, &[1, 2], &one)]);
    assert!(state.holds_picture());
    *file.borrow_mut() = File::Damaged;
    let shown = state
        .step([
            Input::Changed,
            listed(
                PICTURE,
                Fit::Cover,
                &[1, 2],
                &[one[0], (2, surface(1600, 900))],
            ),
        ])
        .unwrap();
    assert_eq!(names(&shown)[&2], "wallpaper/1/cover/1600x900");
    assert_eq!(*decodes.borrow(), 2, "one failed decode");
}

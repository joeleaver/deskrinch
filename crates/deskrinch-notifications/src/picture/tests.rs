use std::path::PathBuf;
use std::sync::Arc;

use deskrinch_notification_service::Image;

use super::*;

fn name(text: &str) -> IconName {
    IconName::parse(text).unwrap()
}

#[test]
fn an_icon_name_comes_first() {
    let icon = ImageSource::Icon(name("mail-unread"));
    assert_eq!(
        Wanted::from_parts(Some(&icon), Some("org.example.Mail")),
        Wanted::Named(name("mail-unread"))
    );
}

#[test]
fn without_one_the_desktop_entry_names_the_application() {
    assert_eq!(
        Wanted::from_parts(None, Some("org.example.Mail")),
        Wanted::Application(DesktopId::new("org.example.Mail.desktop").unwrap())
    );
}

#[test]
fn a_file_or_pixels_are_not_drawn_and_the_entry_stands_in() {
    let file = ImageSource::File(PathBuf::from("/tmp/x.png"));
    let pixels = ImageSource::Pixels(Arc::new(Image::new(1, 1, vec![0, 0, 0, 255]).unwrap()));
    for source in [file, pixels] {
        assert_eq!(Wanted::from_parts(Some(&source), None), Wanted::Nothing);
        assert_eq!(
            Wanted::from_parts(Some(&source), Some("org.example.Mail")),
            Wanted::Application(DesktopId::new("org.example.Mail.desktop").unwrap())
        );
    }
}

#[test]
fn an_entry_that_is_no_desktop_id_is_nothing() {
    assert_eq!(Wanted::from_parts(None, Some("a/b")), Wanted::Nothing);
    assert_eq!(Wanted::from_parts(None, Some("")), Wanted::Nothing);
    assert_eq!(Wanted::from_parts(None, None), Wanted::Nothing);
}

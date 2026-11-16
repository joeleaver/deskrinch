use std::path::PathBuf;
use std::sync::Arc;

use deskrinch_notification_service::Image;

use super::*;

fn name(text: &str) -> IconName {
    IconName::parse(text).unwrap()
}

fn icon(text: &str) -> ImageSource {
    ImageSource::Icon(name(text))
}

fn entry(id: &str) -> Wanted {
    Wanted::Application(DesktopId::new(format!("{id}.desktop")).unwrap())
}

/// A file named by the sender, and pixels it sent: neither drawn.
fn undrawn() -> [ImageSource; 2] {
    [
        ImageSource::File(PathBuf::from("/tmp/x.png")),
        ImageSource::Pixels(Arc::new(Image::new(1, 1, vec![0, 0, 0, 255]).unwrap())),
    ]
}

#[test]
fn the_image_s_icon_name_comes_before_the_app_icon_s() {
    let (image, app) = (icon("dialog-information"), icon("mail-unread"));
    assert_eq!(
        Wanted::from_parts([Some(&image), Some(&app)], Some("org.example.Mail")),
        Wanted::Named(name("dialog-information"))
    );
}

#[test]
fn an_app_icon_name_stands_in_for_an_image_not_drawn() {
    let app = icon("mail-unread");
    for image in undrawn() {
        assert_eq!(
            Wanted::from_parts([Some(&image), Some(&app)], None),
            Wanted::Named(name("mail-unread"))
        );
    }
}

#[test]
fn without_an_icon_name_the_desktop_entry_names_the_application() {
    assert_eq!(
        Wanted::from_parts([None, None], Some("org.example.Mail")),
        entry("org.example.Mail")
    );
    for picture in undrawn() {
        assert_eq!(
            Wanted::from_parts([Some(&picture), Some(&picture)], Some("org.example.Mail")),
            entry("org.example.Mail")
        );
        assert_eq!(
            Wanted::from_parts([None, Some(&picture)], None),
            Wanted::Nothing
        );
    }
}

#[test]
fn an_entry_that_is_no_desktop_id_is_nothing() {
    assert_eq!(
        Wanted::from_parts([None, None], Some("a/b")),
        Wanted::Nothing
    );
    assert_eq!(Wanted::from_parts([None, None], Some("")), Wanted::Nothing);
    assert_eq!(Wanted::from_parts([None, None], None), Wanted::Nothing);
}

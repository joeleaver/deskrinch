//! `Icon` read as GLib's `g_desktop_app_info_load_from_keyfile` reads it
//! (GLib `gio/gdesktopappinfo.c`, read from source for these cases).

use deskrinch_keyfile::Locale;

use super::*;
use crate::entry::{DesktopEntry, DesktopId};

const MAIN: &str = "Desktop Entry";

/// An application whose `Icon` lines are `icon`, and its deviations.
fn read(icon: &str) -> (Option<Localized<Icon>>, Vec<Deviation>) {
    let text = format!("[Desktop Entry]\nType=Application\nName=A\nExec=true\n{icon}\n");
    let id = DesktopId::new("a.desktop").unwrap();
    let entry = DesktopEntry::parse(id, "/a.desktop".into(), text).unwrap();
    (entry.icon, entry.deviations)
}

fn default(icon: &str) -> Option<Icon> {
    with_deviations(icon).0
}

/// The unlocalised icon, and the deviations.
fn with_deviations(icon: &str) -> (Option<Icon>, Vec<Deviation>) {
    let (icon, deviations) = read(icon);
    (icon.map(|icon| icon.default), deviations)
}

fn name(name: &str) -> Icon {
    Icon::Name(name.into())
}

fn extension(value: &str) -> Deviation {
    Deviation::IconExtension {
        group: MAIN.into(),
        value: value.into(),
    }
}

#[test]
fn reads_a_name_and_an_absolute_path() {
    assert_eq!(
        with_deviations("Icon=accessories-text-editor"),
        (Some(name("accessories-text-editor")), vec![])
    );
    let (icon, deviations) = read("Icon=/opt/app/share/app.png");
    assert_eq!(
        icon.unwrap().default,
        Icon::Path("/opt/app/share/app.png".into())
    );
    // A path keeps its extension: it names the file.
    assert_eq!(deviations, []);
}

#[test]
fn strips_each_image_extension_glib_strips() {
    for extension_ in [".png", ".xpm", ".svg"] {
        let value = format!("firefox{extension_}");
        let (icon, deviations) = read(&format!("Icon={value}"));
        assert_eq!(
            icon.map(|icon| icon.default),
            Some(name("firefox")),
            "{value}"
        );
        assert_eq!(deviations, [extension(&value)]);
    }
    // Only the last extension, and only these three, exactly as written:
    // GLib compares the suffix from the last `.` with `strcmp`.
    assert_eq!(default("Icon=a.png.png"), Some(name("a.png")));
    assert_eq!(default("Icon=firefox.PNG"), Some(name("firefox.PNG")));
    assert_eq!(default("Icon=firefox.svgz"), Some(name("firefox.svgz")));
    assert_eq!(default("Icon=firefox.jpg"), Some(name("firefox.jpg")));
}

#[test]
fn keeps_a_name_with_a_dot_whole() {
    assert_eq!(
        with_deviations("Icon=org.gnome.Nautilus"),
        (Some(name("org.gnome.Nautilus")), vec![])
    );
}

#[test]
fn reads_an_empty_value_as_no_icon() {
    assert_eq!(read("Icon="), (None, vec![]));
    // An extension alone strips to nothing: no icon either.
    assert_eq!(read("Icon=.png"), (None, vec![extension(".png")]));
}

#[test]
fn reads_a_relative_path_as_a_name_as_glib_does() {
    let (icon, deviations) = read("Icon=icons/app.png");
    assert_eq!(icon.map(|icon| icon.default), Some(name("icons/app")));
    assert_eq!(
        deviations,
        [
            extension("icons/app.png"),
            Deviation::RelativeIconPath {
                group: MAIN.into(),
                value: "icons/app.png".into()
            }
        ]
    );
}

#[test]
fn reads_each_translation_and_action_icons_the_same_way() {
    let (icon, _) = read("Icon=app.png\nIcon[de]=app-de.svg\nIcon[fr]=");
    let icon = icon.unwrap();
    let de = Locale::parse("de").unwrap();
    let fr = Locale::parse("fr").unwrap();
    assert_eq!(icon.get(&[de]), &Icon::Name("app-de".into()));
    // The empty French translation is dropped: French falls back.
    assert_eq!(icon.get(&[fr]), &Icon::Name("app".into()));

    let text = "[Desktop Entry]\nType=Application\nName=A\nExec=true\nActions=new;\n\
                [Desktop Action new]\nName=New\nExec=true\nIcon=new.xpm\n";
    let id = DesktopId::new("a.desktop").unwrap();
    let entry = DesktopEntry::parse(id, "/a.desktop".into(), text).unwrap();
    let action = &entry.application().unwrap().actions[0];
    assert_eq!(
        action.icon.as_ref().unwrap().default,
        Icon::Name("new".into())
    );
    assert_eq!(
        entry.deviations,
        [Deviation::IconExtension {
            group: "Desktop Action new".into(),
            value: "new.xpm".into()
        }]
    );
}

#[test]
fn passes_the_icon_as_read_to_exec() {
    // GLib strips the extension in place, so `%i` gets the name as read.
    assert_eq!(Icon::Name("firefox".into()).as_os_str(), "firefox");
    assert_eq!(Icon::Path("/a/b.png".into()).as_os_str(), "/a/b.png");
}

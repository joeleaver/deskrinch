use super::*;
use deskrinch_keyfile::Locale;

fn parse(text: &str) -> Result<DesktopEntry, ParseError> {
    parse_for(text, &[])
}

fn parse_for(text: &str, locales: &[Locale]) -> Result<DesktopEntry, ParseError> {
    let id = DesktopId::new("org.example.App.desktop").unwrap();
    DesktopEntry::parse(id, "/x/org.example.App.desktop".into(), text, locales)
}

#[test]
fn ids_come_from_the_relative_path() {
    let id = |p: &str| DesktopId::from_relative_path(Path::new(p)).map(|id| id.0);
    assert_eq!(
        id("kde/konsole.desktop").as_deref(),
        Some("kde-konsole.desktop")
    );
    assert_eq!(id("a.desktop").as_deref(), Some("a.desktop"));
    assert_eq!(id("../a.desktop"), None);
    assert_eq!(id("/a.desktop"), None);
    assert_eq!(id("a.txt"), None);
    assert_eq!(id(".desktop"), None);
    assert_eq!(
        DesktopId::new("org.a.b.desktop").unwrap().app_id(),
        "org.a.b"
    );
}

#[test]
fn parses_an_application() {
    let entry = parse_for(
        "[Desktop Entry]\nType=Application\nName=Editor\nName[de]=Bearbeiter\n\
         GenericName=Text Editor\nComment=Edit\nIcon=accessories-text-editor\n\
         Exec=edit %F\nTryExec=edit\nPath=/tmp\nTerminal=false\nNoDisplay=true\n\
         OnlyShowIn=KDE;GNOME;\nCategories=Utility;TextEditor;\nKeywords=text;notes;\n\
         Keywords[de]=Text;\nMimeType=text/plain;\nStartupWMClass=editor\n\
         StartupNotify=true\nActions=new;\n\
         [Desktop Action new]\nName=New Window\nExec=edit --new\n\
         [Desktop Action unlisted]\nName=Unlisted\nExec=x\n",
        &[Locale::parse("de").unwrap()],
    )
    .unwrap();
    assert_eq!(entry.deviations, []);
    assert_eq!(entry.name.value, "Bearbeiter");
    assert_eq!(entry.name.untranslated.as_deref(), Some("Editor"));
    assert_eq!(entry.generic_name.unwrap().value, "Text Editor");
    assert_eq!(entry.comment.as_deref(), Some("Edit"));
    assert_eq!(
        entry.icon.unwrap(),
        Icon::Name("accessories-text-editor".into())
    );
    assert!(entry.no_display && !entry.hidden);
    assert_eq!(entry.only_show_in, ["KDE", "GNOME"]);
    assert_eq!(entry.categories, ["Utility", "TextEditor"]);
    let keywords = entry.keywords.unwrap();
    assert_eq!(keywords.value, ["Text"]);
    assert_eq!(
        keywords.untranslated,
        Some(vec!["text".to_owned(), "notes".to_owned()])
    );
    let EntryKind::Application(app) = entry.kind else {
        panic!("not an application");
    };
    assert_eq!(app.exec.as_deref(), Some("edit %F"));
    assert_eq!(app.try_exec.as_deref(), Some("edit"));
    assert_eq!(app.working_dir, Some(PathBuf::from("/tmp")));
    assert_eq!(app.mime_types, ["text/plain"]);
    assert_eq!(app.startup_wm_class.as_deref(), Some("editor"));
    assert!(app.startup_notify && !app.terminal && !app.dbus_activatable);
    assert_eq!(app.actions.len(), 1);
    assert_eq!(app.actions[0].id, "new");
    assert_eq!(app.actions[0].name, "New Window");
    assert_eq!(app.actions[0].exec.as_deref(), Some("edit --new"));
}

#[test]
fn parses_links_and_directories() {
    let link = parse("[Desktop Entry]\nType=Link\nName=Site\nURL=https://example.org\n");
    assert_eq!(
        link.unwrap().kind,
        EntryKind::Link {
            url: "https://example.org".into()
        }
    );
    let dir = parse("[Desktop Entry]\nType=Directory\nName=Games\n").unwrap();
    assert_eq!(dir.kind, EntryKind::Directory);
    assert!(dir.application().is_none());
}

#[test]
fn a_dbus_activatable_application_needs_no_exec() {
    let entry = parse(
        "[Desktop Entry]\nType=Application\nName=A\nDBusActivatable=true\nActions=a;\n\
         [Desktop Action a]\nName=Act\n",
    )
    .unwrap();
    assert_eq!(entry.deviations, []);
    let app = entry.application().unwrap();
    assert!(app.dbus_activatable && app.exec.is_none() && app.actions[0].exec.is_none());
}

#[test]
fn rejects_what_cannot_be_an_entry() {
    for (text, key) in [
        ("[Desktop Entry]\nName=A\n", "Type"),
        ("[Desktop Entry]\nType=Link\nName=A\n", "URL"),
    ] {
        assert_eq!(parse(text), Err(ParseError::MissingKey { key }), "{text:?}");
    }
    assert_eq!(
        parse("[Other]\nType=Application\n"),
        Err(ParseError::MissingDesktopEntryGroup)
    );
    assert_eq!(
        parse("[Desktop Entry]\nType=Service\nName=A\n"),
        Err(ParseError::UnknownType {
            value: "Service".into()
        })
    );
}

/// An entry translated into every form of `sr`, `de` and `fr`.
const TRANSLATED: &str = "[Desktop Entry]\nType=Application\nExec=x\n\
    Name=Files\nName[sr]=sr\nName[sr_RS]=sr_RS\nName[sr@latin]=sr@latin\n\
    Name[sr_RS@latin]=sr_RS@latin\nName[de]=Dateien\nName[fr]=Fichiers\n\
    Comment=Browse\nComment[fr]=Parcourir\nKeywords=files;\nKeywords[fr]=fichiers;\n";

#[test]
fn an_entry_is_what_the_file_without_its_skipped_lines_gives() {
    // Only a French name, in a German session: the French line is skipped,
    // and the entry is that of the file with it blanked, a missing `Name`.
    let german = [Locale::parse("de_DE").unwrap()];
    let only_french = "[Desktop Entry]\nType=Application\nName[fr]=Fichiers\nExec=x\n";
    let blanked = "[Desktop Entry]\nType=Application\n\nExec=x\n";
    let entry = parse_for(only_french, &german).unwrap();
    assert_eq!(entry, parse_for(blanked, &german).unwrap());
    let missing_name = Deviation::MissingKey {
        group: MAIN_GROUP.into(),
        key: "Name",
    };
    assert_eq!(entry.deviations, [missing_name]);
    assert_eq!(entry.name.value, UNNAMED);
    // In a French session the line is read, and the whole file's deviation
    // is reported instead: a translation without an untranslated value.
    let french = parse_for(only_french, &[Locale::parse("fr").unwrap()]).unwrap();
    assert_eq!(
        french.deviations,
        [Deviation::KeyFile(
            deskrinch_keyfile::Deviation::TranslationWithoutDefault {
                group: MAIN_GROUP.into(),
                key: "Name".into(),
            }
        )]
    );
    assert_eq!(french.name.value, "Fichiers");
}

#[test]
fn a_value_for_a_locale_not_asked_for_is_not_kept() {
    let entry = parse_for(TRANSLATED, &[Locale::parse("de_DE").unwrap()]).unwrap();
    let kept = format!("{entry:?}");
    for french in ["Fichiers", "Parcourir", "fichiers"] {
        assert!(!kept.contains(french), "{french} kept: {kept}");
    }
    assert_eq!(entry.name.value, "Dateien");
    assert_eq!(entry.comment.as_deref(), Some("Browse"));
    assert_eq!(entry.keywords.unwrap().untranslated, None);
}

#[test]
fn the_resolved_name_is_the_one_the_whole_file_gives() {
    let whole = KeyFile::parse(TRANSLATED.as_bytes(), &mut Vec::<Deviation>::new()).unwrap();
    let name = whole
        .group(MAIN_GROUP)
        .unwrap()
        .localized("Name", &mut Vec::<Deviation>::new())
        .unwrap();
    for wanted in [
        &["sr_RS@latin"][..],
        &["sr_RS"],
        &["sr@latin"],
        &["sr_ME@latin"],
        &["sr_ME"],
        &["de_AT.UTF-8"],
        &["ja", "fr_CA"],
        &["ja"],
        &[],
    ] {
        let locales: Vec<Locale> = wanted.iter().map(|l| Locale::parse(l).unwrap()).collect();
        let entry = parse_for(TRANSLATED, &locales).unwrap();
        assert_eq!(&entry.name.value, name.get(&locales), "{wanted:?}");
    }
}

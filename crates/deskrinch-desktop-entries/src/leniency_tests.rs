//! Files that break the specification, read as GLib reads them.
//!
//! Each case was run through GLib 2.88 (`GKeyFile` with
//! `G_KEY_FILE_KEEP_TRANSLATIONS`, and `g_desktop_app_info_new_from_filename`)
//! with `Exec=true` so the program exists; the comment above each case
//! states what GLib did. A case GLib refuses outright is a `ParseError`
//! here too; a case GLib reads is read, with a `Deviation` recorded.

use super::*;
use crate::locale::Locale;

fn parse(text: impl AsRef<[u8]>) -> Result<DesktopEntry, ParseError> {
    let id = DesktopId::new("a.desktop").unwrap();
    DesktopEntry::parse(id, "/a.desktop".into(), text)
}

/// A conforming application entry with `extra` lines appended.
fn app(extra: &str) -> DesktopEntry {
    parse(format!(
        "[Desktop Entry]\nType=Application\nName=A\nExec=true\n{extra}"
    ))
    .unwrap()
}

fn main(key: &'static str) -> Deviation {
    Deviation::MissingKey {
        group: MAIN_GROUP.into(),
        key,
    }
}

#[test]
fn a_repeated_group_is_merged_with_later_values_winning() {
    // GLib: `[Desktop Entry] Name=A … [Other] … [Desktop Entry]
    // NoDisplay=true` loads, the groups merge and NoDisplay is true; a
    // repeated `Name=B` replaces `Name=A`.
    let entry = app("[Other]\nX=1\n[Desktop Entry]\nNoDisplay=true\nName=B\n");
    assert!(entry.no_display);
    assert_eq!(entry.name.default, "B");
    assert_eq!(
        entry.deviations,
        [
            Deviation::DuplicateGroup {
                group: MAIN_GROUP.into()
            },
            Deviation::DuplicateKey {
                group: MAIN_GROUP.into(),
                key: "Name".into()
            },
        ]
    );
}

#[test]
fn a_repeated_key_takes_the_last_value() {
    // GLib: `Name=A`, `Name=B` gives "B"; `Name[de]=X`, `Name[de]=Y` gives
    // "Y" for de.
    let entry = app("Name=B\nName[de]=X\nName[de]=Y\n");
    assert_eq!(entry.name.default, "B");
    assert_eq!(entry.name.get(&[Locale::parse("de").unwrap()]), "Y");
    assert_eq!(entry.deviations.len(), 2);
}

#[test]
fn booleans_read_as_glib_reads_them() {
    // GLib (`g_key_file_get_boolean`): "1" is true and "0" false;
    // surrounding space is ignored ("true " and " true" are true);
    // "True", "TRUE" and "yes" are errors, which `GDesktopAppInfo` takes
    // as false.
    for (value, read_as, standard) in [
        ("true ", true, true),
        (" true", true, true),
        ("1", true, false),
        ("0", false, false),
        ("True", false, false),
        ("TRUE", false, false),
        ("yes", false, false),
    ] {
        let entry = app(&format!("NoDisplay={value}\n"));
        assert_eq!(entry.no_display, read_as, "{value:?}");
        let expected = if standard {
            vec![]
        } else {
            vec![Deviation::InvalidBoolean {
                group: MAIN_GROUP.into(),
                key: "NoDisplay".into(),
                value: value.trim_start().into(),
                read_as,
            }]
        };
        assert_eq!(entry.deviations, expected, "{value:?}");
    }
    // GLib: `Hidden=1` makes the entry hidden.
    assert!(app("Hidden=1\n").hidden);
}

#[test]
fn nonstandard_key_names_are_kept() {
    // GLib: `K_y=1`, `My Key=1` and `Kéy=1` load; the entry is usable.
    for key in ["K_y", "My Key", "Kéy"] {
        let entry = app(&format!("{key}=1\n"));
        assert_eq!(
            entry.deviations,
            [Deviation::NonstandardKey {
                group: MAIN_GROUP.into(),
                key: key.into()
            }]
        );
    }
}

#[test]
fn a_translation_whose_locale_cannot_match_is_ignored() {
    // GLib: `Name[de.UTF-8]=D` and `Name[]=B` load; neither is the German
    // name.
    let entry = app("Name[de.UTF-8]=D\nName[]=B\n");
    assert_eq!(entry.name.get(&[Locale::parse("de").unwrap()]), "A");
    assert_eq!(entry.deviations.len(), 2);
}

#[test]
fn a_missing_name_reads_as_unnamed_keeping_translations() {
    // GLib: without `Name` the entry loads and `g_app_info_get_name` gives
    // "Unnamed"; with only `Name[de]=D`, the German name is "D".
    let entry = parse("[Desktop Entry]\nType=Application\nExec=true\n").unwrap();
    assert_eq!(entry.name.default, "Unnamed");
    assert_eq!(entry.deviations, [main("Name")]);
    let entry = parse("[Desktop Entry]\nType=Application\nName[de]=D\nExec=true\n").unwrap();
    assert_eq!(entry.name.default, "Unnamed");
    assert_eq!(entry.name.get(&[Locale::parse("de").unwrap()]), "D");
    assert_eq!(
        entry.deviations,
        [Deviation::TranslationWithoutDefault {
            group: MAIN_GROUP.into(),
            key: "Name".into()
        }]
    );
}

#[test]
fn a_missing_exec_leaves_an_entry_with_nothing_to_run() {
    // GLib: an application without `Exec` loads, is shown, and has no
    // command line.
    let entry = parse("[Desktop Entry]\nType=Application\nName=A\n").unwrap();
    assert_eq!(entry.application().unwrap().exec, None);
    assert_eq!(entry.deviations, [main("Exec")]);
}

#[test]
fn actions_missing_keys_are_kept() {
    // GLib: an action group without `Name` is listed as "Unnamed"; one
    // without `Exec` is listed with its name.
    let entry = app("Actions=x;y;\n[Desktop Action x]\nExec=true\n[Desktop Action y]\nName=Y\n");
    let actions = &entry.application().unwrap().actions;
    assert_eq!(actions[0].name.default, "Unnamed");
    assert_eq!(actions[1].exec, None);
    let group = |id: &str| format!("Desktop Action {id}");
    assert_eq!(
        entry.deviations,
        [
            Deviation::MissingKey {
                group: group("x"),
                key: "Name"
            },
            Deviation::MissingKey {
                group: group("y"),
                key: "Exec"
            },
        ]
    );
}

#[test]
fn an_action_without_its_group_is_ignored() {
    // GLib lists it as "Unnamed" with nothing to run; the specification
    // says to ignore it, and that is what happens here.
    let entry = app("Actions=gone;\n");
    assert_eq!(entry.application().unwrap().actions, []);
    assert_eq!(
        entry.deviations,
        [Deviation::ActionWithoutGroup {
            action: "gone".into()
        }]
    );
}

#[test]
fn values_that_are_not_utf8_are_dropped() {
    // GLib: `Name=\xff` loads, the name is unreadable ("Unnamed");
    // `Comment=\xff` loads with no comment.
    let entry = parse(b"[Desktop Entry]\nType=Application\nName=\xff\nExec=true\n").unwrap();
    assert_eq!(entry.name.default, "Unnamed");
    assert_eq!(
        entry.deviations,
        [Deviation::NotUtf8 { line: 3 }, main("Name")]
    );
    let entry = parse(b"[Desktop Entry]\nType=Application\nName=A\nComment=\xff\nExec=true\n");
    assert_eq!(entry.unwrap().comment, None);
}

#[test]
fn undefined_escapes_are_kept_literally() {
    // GLib refuses to read `Name=A\$x` (and a value ending in `\`) and
    // drops an entry whose `Exec` it cannot read. Here the backslash is kept
    // (see `keyfile::unescape`), so `Exec=sh -c "echo \$HOME"` still runs.
    let entry = app("Comment=A\\$x\n");
    assert_eq!(entry.comment.unwrap().default, "A\\$x");
    assert_eq!(
        entry.deviations,
        [Deviation::InvalidEscape {
            group: MAIN_GROUP.into(),
            key: "Comment".into()
        }]
    );
}

#[test]
fn leading_space_and_crlf_are_read_silently() {
    // GLib: a key line indented with spaces, and CRLF line ends, load
    // without complaint.
    let entry = parse("[Desktop Entry]\r\nType=Application\r\n  Name=A\r\nExec=true\r\n").unwrap();
    assert_eq!(entry.name.default, "A");
    assert_eq!(entry.deviations, []);
}

#[test]
fn what_glib_refuses_is_refused() {
    // GLib: each of these fails to load (`GKeyFile` errors, or
    // `GDesktopAppInfo` returns NULL).
    for (text, expected) in [
        // "Key file does not start with a group".
        (
            "Foo=bar\n[Desktop Entry]\nType=Application\nName=A\nExec=true\n",
            ParseError::KeyOutsideGroup { line: 1 },
        ),
        // `GDesktopAppInfo` requires `Desktop Entry` as the start group.
        (
            "[Other]\nX=1\n[Desktop Entry]\nType=Application\nName=A\nExec=true\n",
            ParseError::MissingDesktopEntryGroup,
        ),
        // "…which is not a key-value pair, group, or comment".
        (
            "[Desktop Entry]\nType=Application\nName=A\nExec=true\njunk line\n",
            ParseError::InvalidLine { line: 5 },
        ),
        // "Invalid key name: Name[d e]".
        (
            "[Desktop Entry]\nType=Application\nName[d e]=A\nExec=true\n",
            ParseError::InvalidLine { line: 3 },
        ),
        // No `Type`, or an unknown one, and `GDesktopAppInfo` returns NULL.
        (
            "[Desktop Entry]\nName=A\nExec=true\n",
            ParseError::MissingKey { key: "Type" },
        ),
        (
            "[Desktop Entry]\nType=\nName=A\nExec=true\n",
            ParseError::UnknownType {
                value: String::new(),
            },
        ),
    ] {
        assert_eq!(parse(text), Err(expected), "{text:?}");
    }
}

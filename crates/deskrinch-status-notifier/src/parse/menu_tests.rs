//! Turning a layout as sent into a menu: each field cleaned and bounded on
//! its own.

use std::path::PathBuf;

use proptest::prelude::*;

use super::menu::{label, menu};
use crate::item::ItemId;
use crate::limits::Limits;
use crate::menu::*;
use crate::wire::layout::{RawEntry, RawMenuProperties, RawProperties};
use deskrinch_untrusted::fuzz::config;

fn limits() -> Limits {
    Limits {
        max_text_bytes: 16,
        max_reference_bytes: 24,
        max_menu_icon_bytes: 1024,
        max_image_side: 8,
        max_shortcuts: 2,
        ..Limits::default()
    }
}

fn raw(id: i32, properties: RawProperties) -> RawEntry {
    RawEntry {
        id,
        properties,
        children: Vec::new(),
    }
}

fn entries_of(children: Vec<RawEntry>) -> Vec<MenuEntry> {
    let root = RawEntry {
        id: 0,
        properties: RawProperties::default(),
        children,
    };
    menu(item(), root, RawMenuProperties::default(), &limits()).entries
}

fn item() -> ItemId {
    ItemId::new(":1.1", "/StatusNotifierItem")
}

fn one(properties: RawProperties) -> MenuEntry {
    entries_of(vec![raw(1, properties)]).remove(0)
}

/// A PNG of `side` × `side` pixels.
fn png(side: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, side, side);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![7; (side * side * 4) as usize])
            .unwrap();
    }
    out
}

#[test]
fn an_entry_with_nothing_set_has_the_protocol_s_defaults() {
    let entry = one(RawProperties::default());
    assert_eq!(entry.id, EntryId { id: 1, since: 0 });
    assert_eq!(entry.kind, EntryKind::Standard);
    assert_eq!(entry.label, Label::default());
    assert!(entry.enabled);
    assert!(entry.icon.is_empty());
    assert_eq!(entry.toggle, Toggle::None);
    assert_eq!(entry.shortcuts, Vec::new());
    assert_eq!(entry.disposition, Disposition::Normal);
    assert_eq!(entry.submenu, None);
}

#[test]
fn underscores_mark_the_mnemonic_and_double_ones_are_literal() {
    let limits = Limits::default();
    let cases = [
        ("_Open", "Open", Some(0)),
        ("Save __as", "Save _as", None),
        ("E_xit", "Exit", Some(1)),
        ("_a_b", "ab", Some(0)),
        ("tail_", "tail_", None),
        ("Fü_r", "Für", Some(3)),
        ("", "", None),
    ];
    for (raw, text, mnemonic) in cases {
        let label = label(raw, &limits);
        assert_eq!(
            (label.text.as_str(), label.mnemonic),
            (text, mnemonic),
            "{raw:?}"
        );
    }
    // Text, not markup; control characters go, and the length is bounded.
    let label = label("<b>a</b>\u{7}\nb", &limits);
    assert_eq!(label.text, "<b>a</b> b");
    assert_eq!(
        super::menu::label(&"x".repeat(100), &self::limits())
            .text
            .len(),
        16
    );
}

#[test]
fn hidden_entries_go_with_their_submenus_and_separators_stay_as_sent() {
    let hidden = RawProperties {
        visible: Some(false),
        ..RawProperties::default()
    };
    let separator = RawProperties {
        kind: Some("separator".into()),
        label: Some("ignored".into()),
        ..RawProperties::default()
    };
    let mut submenu = raw(2, hidden);
    submenu.children = vec![raw(3, RawProperties::default())];
    let entries = entries_of(vec![
        raw(1, separator.clone()),
        submenu,
        raw(4, separator.clone()),
        raw(5, separator),
    ]);
    let kinds: Vec<_> = entries.iter().map(|e| (e.id.id, e.kind)).collect();
    assert_eq!(
        kinds,
        [
            (1, EntryKind::Separator),
            (4, EntryKind::Separator),
            (5, EntryKind::Separator),
        ]
    );
    assert_eq!(entries[0].label, Label::default());
}

#[test]
fn a_submenu_is_one_that_says_so_or_has_children() {
    let says = one(RawProperties {
        children_display: Some("submenu".into()),
        ..RawProperties::default()
    });
    assert_eq!(says.submenu, Some(Vec::new()));
    let mut parent = raw(1, RawProperties::default());
    parent.children = vec![raw(2, RawProperties::default())];
    let has = entries_of(vec![parent]).remove(0);
    assert_eq!(has.submenu.unwrap()[0].id.id, 2);
}

#[test]
fn toggles_read_their_kind_and_state() {
    let toggle = |kind: Option<&str>, state| {
        one(RawProperties {
            toggle_type: kind.map(str::to_owned),
            toggle_state: state,
            ..RawProperties::default()
        })
        .toggle
    };
    assert_eq!(
        toggle(Some("checkmark"), Some(1)),
        Toggle::Checkmark(ToggleState::On)
    );
    assert_eq!(
        toggle(Some("radio"), Some(0)),
        Toggle::Radio(ToggleState::Off)
    );
    assert_eq!(
        toggle(Some("checkmark"), Some(-1)),
        Toggle::Checkmark(ToggleState::Indeterminate)
    );
    assert_eq!(
        toggle(Some("radio"), None),
        Toggle::Radio(ToggleState::Indeterminate)
    );
    assert_eq!(toggle(Some("switch"), Some(1)), Toggle::None);
    assert_eq!(toggle(None, Some(1)), Toggle::None);
}

#[test]
fn shortcuts_are_cleaned_and_beyond_their_count_dropped() {
    let keys = |k: &[&str]| k.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let entry = one(RawProperties {
        shortcut: Some(vec![
            keys(&["Control", "q"]),
            keys(&[]),
            keys(&["Control", "Alt", "Shift", "Super", "x", "y"]),
            keys(&["Alt", " "]),
            keys(&["F1"]),
            keys(&["F2"]),
        ]),
        ..RawProperties::default()
    });
    let shown: Vec<_> = entry.shortcuts.iter().map(|s| s.keys.clone()).collect();
    assert_eq!(shown, [keys(&["Control", "q"]), keys(&["F1"])]);
}

#[test]
fn icons_are_named_decoded_or_dropped() {
    let icon = |name: Option<&str>, data: Option<Vec<u8>>| {
        one(RawProperties {
            icon_name: name.map(str::to_owned),
            icon_data: data,
            ..RawProperties::default()
        })
        .icon
    };
    let both = icon(Some("document-open"), Some(png(4)));
    assert_eq!(both.name.unwrap().as_str(), "document-open");
    assert_eq!(both.pixmaps[0].width(), 4);
    // Too wide, too many bytes, not a PNG, a path for a name.
    assert!(icon(None, Some(png(9))).is_empty());
    let mut large = png(4);
    large.resize(2048, 0);
    assert!(icon(None, Some(large)).is_empty());
    assert!(icon(None, Some(b"not a png".to_vec())).is_empty());
    assert!(icon(Some("../../etc/passwd"), None).is_empty());
}

#[test]
fn dispositions_and_the_menu_s_own_properties_are_read() {
    let disposition = |d: &str| {
        one(RawProperties {
            disposition: Some(d.into()),
            ..RawProperties::default()
        })
        .disposition
    };
    assert_eq!(disposition("alert"), Disposition::Alert);
    assert_eq!(disposition("warning"), Disposition::Warning);
    assert_eq!(disposition("informative"), Disposition::Informative);
    assert_eq!(disposition("loud"), Disposition::Normal);
    let properties = RawMenuProperties {
        text_direction: Some("rtl".into()),
        icon_theme_path: Some(vec![
            "/a/icons".into(),
            "relative".into(),
            "/b".into(),
            "/c/is/past/the/bound/on/references".into(),
        ]),
    };
    let menu = menu(
        item(),
        raw(0, RawProperties::default()),
        properties,
        &limits(),
    );
    assert_eq!(menu.text_direction, TextDirection::RightToLeft);
    assert_eq!(
        menu.icon_theme_path,
        [PathBuf::from("/a/icons"), PathBuf::from("/b")]
    );
}

fn properties() -> impl Strategy<Value = RawProperties> {
    let text = || proptest::option::of(any::<String>());
    (
        (text(), text(), proptest::option::of(any::<bool>()), text()),
        (
            proptest::option::of(prop::collection::vec(any::<u8>(), 0..2048)),
            text(),
            proptest::option::of(any::<i32>()),
            text(),
        ),
        proptest::option::of(prop::collection::vec(
            prop::collection::vec(any::<String>(), 0..8),
            0..8,
        )),
    )
        .prop_map(
            |(
                (kind, label, enabled, icon_name),
                (icon_data, toggle_type, toggle_state, disposition),
                shortcut,
            )| {
                RawProperties {
                    kind,
                    label,
                    enabled,
                    visible: None,
                    icon_name,
                    icon_data,
                    toggle_type,
                    toggle_state,
                    children_display: None,
                    disposition,
                    shortcut,
                }
            },
        )
}

proptest! {
    #![proptest_config(config())]

    /// Whatever an entry sends, it reads within its bounds.
    #[test]
    fn any_entry_reads_within_its_bounds(properties in properties()) {
        let limits = limits();
        let entry = one(properties);
        prop_assert!(entry.label.text.len() <= limits.max_text_bytes);
        prop_assert!(!entry.label.text.chars().any(char::is_control));
        if let Some(at) = entry.label.mnemonic {
            prop_assert!(entry.label.text.is_char_boundary(at) && at < entry.label.text.len());
        }
        prop_assert!(entry.shortcuts.len() <= limits.max_shortcuts);
        for shortcut in &entry.shortcuts {
            prop_assert!(!shortcut.keys.is_empty() && shortcut.keys.len() <= 5);
            for key in &shortcut.keys {
                prop_assert!(!key.is_empty() && key.len() <= limits.max_text_bytes);
            }
        }
        prop_assert!(entry.icon.pixmaps.len() <= 1);
        for p in &entry.icon.pixmaps {
            prop_assert!(p.width() <= limits.max_image_side && p.height() <= limits.max_image_side);
        }
    }
}

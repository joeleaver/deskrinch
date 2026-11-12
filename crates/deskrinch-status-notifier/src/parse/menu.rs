//! From a layout as sent to a [`Menu`]: hidden entries left out, labels
//! cleaned and their mnemonics found, icons checked and decoded, each
//! field bounded on its own. The structural bounds were held as the layout
//! was read ([`crate::wire::layout`]).

use std::path::PathBuf;
use std::sync::Arc;

use deskrinch_untrusted::text::{self, Lines};
use deskrinch_untrusted::{IconName, Image};

use crate::item::{Icon, ItemId};
use crate::limits::Limits;
use crate::menu::{
    Disposition, EntryId, EntryKind, Label, Menu, MenuEntry, Shortcut, TextDirection, Toggle,
    ToggleState,
};
use crate::wire::layout::{RawEntry, RawMenuProperties, RawProperties};

/// Keys of one shortcut: the four modifiers and the key. A shortcut with
/// more is not one an application means, and is dropped.
const MAX_KEYS: usize = 5;

/// The menu of item `item` whose root entry is `root`, within `limits`.
/// Every entry's id names the reading it came in as `0`; the model gives
/// each the reading it has been the same since.
pub(crate) fn menu(
    item: ItemId,
    root: RawEntry,
    properties: RawMenuProperties,
    limits: &Limits,
) -> Menu {
    Menu {
        item,
        entries: entries(root.children, limits),
        text_direction: match properties.text_direction.as_deref() {
            Some("rtl") => TextDirection::RightToLeft,
            _ => TextDirection::LeftToRight,
        },
        icon_theme_path: theme_paths(properties.icon_theme_path.unwrap_or_default(), limits),
    }
}

fn entries(raw: Vec<RawEntry>, limits: &Limits) -> Vec<MenuEntry> {
    raw.into_iter()
        .filter_map(|entry| self::entry(entry, limits))
        .collect()
}

/// The entry `raw`; `None` when it is hidden.
fn entry(raw: RawEntry, limits: &Limits) -> Option<MenuEntry> {
    let RawEntry {
        id,
        properties,
        children,
    } = raw;
    let RawProperties {
        kind,
        label,
        enabled,
        visible,
        icon_name,
        icon_data,
        toggle_type,
        toggle_state,
        children_display,
        disposition,
        shortcut,
    } = properties;
    if visible == Some(false) {
        return None;
    }
    let kind = match kind.as_deref() {
        Some("separator") => EntryKind::Separator,
        _ => EntryKind::Standard,
    };
    let has_submenu = children_display.as_deref() == Some("submenu") || !children.is_empty();
    Some(MenuEntry {
        id: EntryId { id, since: 0 },
        kind,
        label: match (kind, label) {
            (EntryKind::Standard, Some(label)) => self::label(&label, limits),
            _ => Label::default(),
        },
        enabled: enabled.unwrap_or(true),
        icon: icon(icon_name.as_deref(), icon_data.as_deref(), limits),
        toggle: toggle(toggle_type.as_deref(), toggle_state),
        shortcuts: shortcut
            .unwrap_or_default()
            .into_iter()
            .filter_map(|keys| self::shortcut(&keys, limits))
            .take(limits.max_shortcuts)
            .collect(),
        disposition: match disposition.as_deref() {
            Some("informative") => Disposition::Informative,
            Some("warning") => Disposition::Warning,
            Some("alert") => Disposition::Alert,
            _ => Disposition::Normal,
        },
        submenu: has_submenu.then(|| entries(children, limits)),
    })
}

/// `raw` cleaned and bounded, its underscores read: `__` is one
/// underscore, and `_` before another character marks that character,
/// the first one so marked being the mnemonic. A trailing `_` stays.
pub(crate) fn label(raw: &str, limits: &Limits) -> Label {
    let cleaned = text::clean(raw, limits.max_text_bytes, Lines::Join);
    let mut label = Label::default();
    let mut chars = cleaned.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('_', Some('_')) => {
                chars.next();
                label.text.push('_');
            }
            ('_', Some(&next)) => {
                chars.next();
                label.mnemonic.get_or_insert(label.text.len());
                label.text.push(next);
            }
            (c, _) => label.text.push(c),
        }
    }
    label
}

fn icon(name: Option<&str>, png: Option<&[u8]>, limits: &Limits) -> Icon {
    Icon {
        name: name
            .filter(|n| n.len() <= limits.max_reference_bytes)
            .and_then(IconName::parse),
        pixmaps: png
            .filter(|png| png.len() <= limits.max_menu_icon_bytes)
            .and_then(|png| Image::from_png(png, limits.max_image_side))
            .map(Arc::new)
            .into_iter()
            .collect(),
    }
}

fn toggle(kind: Option<&str>, state: Option<i32>) -> Toggle {
    let state = match state {
        Some(0) => ToggleState::Off,
        Some(1) => ToggleState::On,
        _ => ToggleState::Indeterminate,
    };
    match kind {
        Some("checkmark") => Toggle::Checkmark(state),
        Some("radio") => Toggle::Radio(state),
        _ => Toggle::None,
    }
}

/// One shortcut's keys, each cleaned and bounded; `None` when it has none,
/// more than a shortcut can, or an empty one.
fn shortcut(keys: &[String], limits: &Limits) -> Option<Shortcut> {
    if keys.is_empty() || keys.len() > MAX_KEYS {
        return None;
    }
    let keys = keys
        .iter()
        .map(|key| text::non_empty(key, limits.max_text_bytes))
        .collect::<Option<Vec<_>>>()?;
    Some(Shortcut { keys })
}

/// The directories of `IconThemePath` that are valid ones, in order, while
/// they fit within the bound on references together.
fn theme_paths(paths: Vec<String>, limits: &Limits) -> Vec<PathBuf> {
    let mut left = limits.max_reference_bytes;
    paths
        .into_iter()
        .take_while(|p| {
            let fits = p.len() <= left;
            left = left.saturating_sub(p.len());
            fits
        })
        .filter_map(|p| super::theme_path(&p, limits))
        .collect()
}

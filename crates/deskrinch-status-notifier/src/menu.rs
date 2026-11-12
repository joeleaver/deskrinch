//! What the host reports about an item's open menu (`com.canonical.dbusmenu`):
//! its entries, as the user is to see them.

use std::fmt;
use std::path::PathBuf;

use crate::item::{Icon, ItemId};

/// An item's menu, as read while the caller has it open
/// ([`Handle::open_menu`](crate::Handle::open_menu)).
///
/// Every string in it is cleaned and bounded, and every icon validated,
/// as an item's are ([`Limits`](crate::Limits)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    /// The item whose menu it is.
    pub item: ItemId,
    /// The menu's entries, in order.
    pub entries: Vec<MenuEntry>,
    /// Which way the menu's text runs.
    pub text_direction: TextDirection,
    /// Directories to look in for the entries' named icons before the icon
    /// theme, in order (`IconThemePath`): absolute paths only.
    pub icon_theme_path: Vec<PathBuf>,
}

/// Names one entry of an open menu, as the caller was given it.
///
/// It names more than the application's number for the entry: it also
/// carries which reading of the menu that entry has been the same since
/// (the same kind, label, toggle kind, and submenu or not). Some
/// applications number entries by their place in the menu, so after the
/// menu changes the same number can name another entry; an id given
/// before the change then names nothing, and the host refuses to activate
/// it ([`OperationError::EntryUnavailable`](crate::OperationError)) rather
/// than activate what the user did not see. An entry whose label alone
/// changed is a different entry; an entry whose other properties changed
/// (enabled, its icon, its toggle state) is the same.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryId {
    pub(crate) id: i32,
    pub(crate) since: u64,
}

impl fmt::Debug for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EntryId({}@{})", self.id, self.since)
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.id)
    }
}

/// One entry of a menu.
///
/// Entries come as the application sent them, apart from hidden ones,
/// which are left out with everything under them: separators in
/// particular are not tidied (a leading, trailing or doubled separator is
/// passed on), which the UI may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    /// The entry, for [`Handle::activate_entry`](crate::Handle::activate_entry)
    /// and [`Handle::open_submenu`](crate::Handle::open_submenu).
    pub id: EntryId,
    /// An entry, or a separator.
    pub kind: EntryKind,
    /// What it says; empty for a separator.
    pub label: Label,
    /// Whether it can be activated now.
    pub enabled: bool,
    /// Its icon, as a themed name, as pixels, or both; pixels come from the
    /// application's PNG, decoded.
    pub icon: Icon,
    /// Whether it is a checkbox or a radio button, and in which state.
    pub toggle: Toggle,
    /// The keyboard shortcuts the application gives it, for display only:
    /// the host binds no key.
    pub shortcuts: Vec<Shortcut>,
    /// How the application would like it shown.
    pub disposition: Disposition,
    /// Its submenu, if it opens one: in order, and possibly empty until it
    /// is opened ([`Handle::open_submenu`](crate::Handle::open_submenu)),
    /// as many applications fill a submenu only then.
    pub submenu: Option<Vec<MenuEntry>>,
}

/// An entry or a separator (the entry's `type`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// An entry that can be activated. A type the application does not
    /// name, or names with another value, reads as this.
    #[default]
    Standard,
    /// A line between entries.
    Separator,
}

/// An entry's label: text, not markup (the protocol has none).
///
/// Applications mark the mnemonic with an underscore (`_Open`) and write
/// a literal underscore as two (`Save __as`); the underscores are taken
/// out of [`text`](Self::text) and the first marked character noted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Label {
    /// The label as it is to be read, cleaned of control characters.
    pub text: String,
    /// The byte offset in [`text`](Self::text) of the mnemonic character,
    /// if the label marks one.
    pub mnemonic: Option<usize>,
}

/// Whether an entry is a checkbox or a radio button
/// (`toggle-type` and `toggle-state`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Toggle {
    /// Neither. A toggle type the application does not name, or names with
    /// another value, reads as this.
    #[default]
    None,
    /// A checkbox.
    Checkmark(ToggleState),
    /// One of a group of radio buttons.
    Radio(ToggleState),
}

/// A checkbox's or radio button's state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ToggleState {
    /// Not checked (`0`).
    Off,
    /// Checked (`1`).
    On,
    /// Neither: any other value, or none.
    #[default]
    Indeterminate,
}

/// One keyboard shortcut, for display: its keys, in order, as the
/// application names them, each cleaned and bounded. The specification
/// names the modifiers `Control`, `Alt`, `Shift` and `Super`, and the key
/// follows (`q`, `F1`); Qt adds `Num` for the keypad, and names keys as Qt
/// does (`plus`, `minus`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Shortcut {
    /// The keys pressed together.
    pub keys: Vec<String>,
}

/// How the application would like an entry shown (`disposition`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Disposition {
    /// As any entry. A value the application does not name, or names with
    /// another value, reads as this.
    #[default]
    Normal,
    /// As information.
    Informative,
    /// As a warning.
    Warning,
    /// As an alert.
    Alert,
}

/// Which way a menu's text runs (`TextDirection`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum TextDirection {
    /// Left to right. A direction the menu does not give, or gives with
    /// another value, reads as this.
    #[default]
    LeftToRight,
    /// Right to left.
    RightToLeft,
}

/// Why an open menu could not be read
/// ([`Event::MenuFailed`](crate::Event::MenuFailed)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum MenuFailure {
    /// The item no longer exports a menu.
    #[error("the item has no menu")]
    NoMenu,
    /// The application did not answer in time.
    #[error("the menu did not answer in time")]
    TimedOut,
    /// The reply was larger than its bound.
    #[error("the menu's reply is too large")]
    TooLarge,
    /// The reply was not a menu layout.
    #[error("the menu's reply is not a layout")]
    Malformed,
    /// The layout was deeper, or had more entries, than its bounds.
    #[error("the menu is larger than its bounds")]
    OverBound,
    /// The application answered with an error.
    #[error("the application refused to give its menu")]
    Refused,
}

impl MenuEntry {
    /// Every entry of `entries`, submenus included, depth first.
    pub(crate) fn walk(entries: &[Self]) -> impl Iterator<Item = &Self> {
        let mut stack: Vec<&Self> = entries.iter().rev().collect();
        std::iter::from_fn(move || {
            let entry = stack.pop()?;
            if let Some(submenu) = &entry.submenu {
                stack.extend(submenu.iter().rev());
            }
            Some(entry)
        })
    }
}

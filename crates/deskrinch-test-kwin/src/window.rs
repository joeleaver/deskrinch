//! A window as KWin holds it: what its script API gives of it, typed.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::Error;

/// A window's type, as KWin holds it (its `windowType`, KWin's own
/// numbering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WindowType {
    /// An ordinary window; also a layer surface whose namespace KWin does
    /// not know.
    Normal,
    /// The desktop: the wallpaper's window.
    Desktop,
    /// A dock: a panel's.
    Dock,
    /// A toolbar.
    Toolbar,
    /// A menu.
    Menu,
    /// A dialog.
    Dialog,
    /// A utility window.
    Utility,
    /// A splash screen.
    Splash,
    /// A drop-down menu.
    DropdownMenu,
    /// A pop-up menu.
    PopupMenu,
    /// A tooltip.
    Tooltip,
    /// A notification.
    Notification,
    /// A combo box's list.
    ComboBox,
    /// A drag-and-drop icon.
    DndIcon,
    /// An on-screen display.
    OnScreenDisplay,
    /// A critical notification.
    CriticalNotification,
    /// An applet's pop-up.
    AppletPopup,
    /// A number KWin gives that this crate does not name.
    Other(i64),
}

impl WindowType {
    /// The type KWin numbers `number` (`NET::WindowType`, which KWin's
    /// `windowType` property gives).
    #[must_use]
    pub fn from_kwin(number: i64) -> Self {
        match number {
            0 => Self::Normal,
            1 => Self::Desktop,
            2 => Self::Dock,
            3 => Self::Toolbar,
            4 => Self::Menu,
            5 => Self::Dialog,
            8 => Self::Utility,
            9 => Self::Splash,
            10 => Self::DropdownMenu,
            11 => Self::PopupMenu,
            12 => Self::Tooltip,
            13 => Self::Notification,
            14 => Self::ComboBox,
            15 => Self::DndIcon,
            16 => Self::OnScreenDisplay,
            17 => Self::CriticalNotification,
            18 => Self::AppletPopup,
            other => Self::Other(other),
        }
    }
}

/// The window types KWin also states as flags, by the script property
/// that states each: a second statement of the type.
pub(crate) const FLAGS: [(&str, WindowType); 17] = [
    ("normalWindow", WindowType::Normal),
    ("desktopWindow", WindowType::Desktop),
    ("dock", WindowType::Dock),
    ("toolbar", WindowType::Toolbar),
    ("menu", WindowType::Menu),
    ("dialog", WindowType::Dialog),
    ("utility", WindowType::Utility),
    ("splash", WindowType::Splash),
    ("dropdownMenu", WindowType::DropdownMenu),
    ("popupMenu", WindowType::PopupMenu),
    ("tooltip", WindowType::Tooltip),
    ("notification", WindowType::Notification),
    ("comboBox", WindowType::ComboBox),
    ("dndIcon", WindowType::DndIcon),
    ("onScreenDisplay", WindowType::OnScreenDisplay),
    ("criticalNotification", WindowType::CriticalNotification),
    ("appletPopup", WindowType::AppletPopup),
];

/// What leaves a window out, by the script property that says so.
pub(crate) const SKIPS: [(&str, Skip); 3] = [
    ("skipTaskbar", Skip::Taskbar),
    ("skipPager", Skip::Pager),
    ("skipSwitcher", Skip::Switcher),
];

/// The other properties a record holds as KWin gives them.
pub(crate) const PROPERTIES: [&str; 6] = [
    "caption",
    "resourceClass",
    "resourceName",
    "windowType",
    "layer",
    "pid",
];

/// What leaves a window out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Skip {
    /// A taskbar.
    Taskbar,
    /// A pager.
    Pager,
    /// A window switcher.
    Switcher,
}

/// A window, as KWin holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// Its caption: a toplevel's title; empty for a layer surface.
    pub caption: String,
    /// Its resource class: a toplevel's app id; a layer surface's process
    /// name.
    pub resource_class: String,
    /// Its resource name: its process's name.
    pub resource_name: String,
    /// Its type, by KWin's number.
    pub window_type: WindowType,
    /// The types whose flag KWin sets for it (`normalWindow`, `dock`, …): a
    /// second statement of its type, which a test may hold against the
    /// first.
    pub flags: BTreeSet<WindowType>,
    /// Its layer, as KWin numbers its stacking layers (2 for an ordinary
    /// window, 9 for an overlay layer surface, on KWin 6.6.7).
    pub layer: i64,
    /// The output it is on, by name.
    pub output: Option<String>,
    /// Its frame's geometry: x, y, width and height, in logical pixels.
    pub frame: [f64; 4],
    /// What leaves it out.
    pub skips: BTreeSet<Skip>,
    /// Its client's process id.
    pub pid: u32,
}

impl Window {
    /// Its frame's width and height.
    #[must_use]
    pub fn size(&self) -> (f64, f64) {
        (self.frame[2], self.frame[3])
    }

    /// The window `record`, a record as the script writes it (an object of
    /// KWin's property names, a rectangle as `[x, y, width, height]` and the
    /// output by name), describes.
    pub(crate) fn from_record(record: &str) -> Result<Self, Error> {
        let fields = Fields::parse(record)?;
        let flags = FLAGS
            .into_iter()
            .filter_map(|(name, kind)| fields.flag(name).map(|set| set.then_some(kind)).transpose())
            .collect::<Result<_, _>>()?;
        let skips = SKIPS
            .into_iter()
            .filter_map(|(name, skip)| fields.flag(name).map(|set| set.then_some(skip)).transpose())
            .collect::<Result<_, _>>()?;
        let output = match fields.field("output")? {
            Value::Null => None,
            Value::String(name) => Some(name.clone()),
            _ => return Err(fields.refuse("output is neither a name nor null")),
        };
        let frame: [f64; 4] = serde_json::from_value(fields.field("frameGeometry")?.clone())
            .map_err(|error| fields.refuse(&format!("frameGeometry: {error}")))?;
        let pid = u32::try_from(fields.integer("pid")?)
            .map_err(|_| fields.refuse("pid is out of range"))?;
        Ok(Self {
            caption: fields.text("caption")?,
            resource_class: fields.text("resourceClass")?,
            resource_name: fields.text("resourceName")?,
            window_type: WindowType::from_kwin(fields.integer("windowType")?),
            flags,
            layer: fields.integer("layer")?,
            output,
            frame,
            skips,
            pid,
        })
    }
}

/// A record's fields, by KWin's property names.
struct Fields<'a> {
    record: &'a str,
    fields: Map<String, Value>,
}

impl<'a> Fields<'a> {
    fn parse(record: &'a str) -> Result<Self, Error> {
        let fields = serde_json::from_str(record).map_err(|error| Error::Record {
            record: record.to_owned(),
            reason: error.to_string(),
        })?;
        Ok(Self { record, fields })
    }

    fn refuse(&self, reason: &str) -> Error {
        Error::Record {
            record: self.record.to_owned(),
            reason: reason.to_owned(),
        }
    }

    fn field(&self, name: &str) -> Result<&Value, Error> {
        self.fields
            .get(name)
            .ok_or_else(|| self.refuse(&format!("no {name}")))
    }

    fn text(&self, name: &str) -> Result<String, Error> {
        self.field(name)?
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| self.refuse(&format!("{name} is not text")))
    }

    fn integer(&self, name: &str) -> Result<i64, Error> {
        self.field(name)?
            .as_i64()
            .ok_or_else(|| self.refuse(&format!("{name} is not an integer")))
    }

    fn flag(&self, name: &str) -> Result<bool, Error> {
        self.field(name)?
            .as_bool()
            .ok_or_else(|| self.refuse(&format!("{name} is not true or false")))
    }
}

#[cfg(test)]
mod tests;

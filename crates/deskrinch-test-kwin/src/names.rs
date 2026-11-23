//! KWin's names on its bus, each in one place: the service, and each
//! object with its interface that this crate asks or acts on.

/// KWin's name on the bus.
pub(crate) const KWIN: &str = "org.kde.KWin";

/// KWin's own object and interface: its support information, its current
/// desktop.
pub(crate) const KWIN_OBJECT: (&str, &str) = ("/KWin", "org.kde.KWin");

/// KWin's scripting object and interface.
pub(crate) const SCRIPTING: (&str, &str) = ("/Scripting", "org.kde.kwin.Scripting");

/// A loaded script's interface (its object is `/Scripting/Script<id>`).
pub(crate) const SCRIPT: &str = "org.kde.kwin.Script";

/// KWin's effects object and interface.
pub(crate) const EFFECTS: (&str, &str) = ("/Effects", "org.kde.kwin.Effects");

/// KWin's virtual desktop manager's object and interface.
pub(crate) const MANAGER: (&str, &str) = (
    "/VirtualDesktopManager",
    "org.kde.KWin.VirtualDesktopManager",
);

/// KWin's shortcut service, its component for KWin's own shortcuts, and
/// that component's interface. On a nested KWin's bus KWin itself owns the
/// service's name (`shortcuts`' doc says where that was checked).
pub(crate) const SHORTCUTS: (&str, &str, &str) = (
    "org.kde.kglobalaccel",
    "/component/kwin",
    "org.kde.kglobalaccel.Component",
);

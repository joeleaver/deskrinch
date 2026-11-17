//! What deskrinch's shell consists of, in one place: its components, in the
//! order they start, what each one's being ready means, what each must be
//! granted and when each is started again ([`COMPONENTS`], [`restart`]),
//! the statuses a component ends with besides success ([`exit_status`]), and
//! the shell's own key bindings ([`LAUNCHER_CHORD`]).
//!
//! `cargo xtask desktop` starts the shell from this, in a nested compositor
//! in a window, grants each component its restricted interfaces first, and
//! starts a component that fails again as its policy says; the real login
//! session will start it, grant them and restart them from this too. This
//! crate is data, and the rules on it: no I/O and no processes. How a
//! component is built and started, and how a compositor is told about a key
//! binding, are each starter's own.
//!
//! # Example
//!
//! ```
//! use deskrinch_shell::{COMPONENTS, Ready};
//!
//! // The wallpaper starts first; the launcher, hidden, is ready once it
//! // owns its name.
//! assert_eq!(COMPONENTS[0].package, "deskrinch-wallpaper");
//! let launcher = COMPONENTS
//!     .iter()
//!     .find(|component| component.package == "deskrinch-launcher")
//!     .unwrap();
//! assert_eq!(launcher.ready, Ready::BusName("org.deskrinch.Launcher"));
//! ```

pub mod exit_status;
pub mod restart;

pub use restart::Restart;

/// One component of the shell: a program the session runs for as long as it
/// lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    /// The Cargo package, whose binary of the same name is the component.
    pub package: &'static str,
    /// Its arguments.
    pub args: &'static [&'static str],
    /// When the next component may start.
    pub ready: Ready,
    /// The restricted Wayland interfaces it uses, by their protocol names:
    /// what a compositor that offers them only to the programs it grants
    /// them must grant it (KWin, through the program's installed desktop
    /// file). Every starter grants them before it starts the component.
    pub wayland_grants: &'static [&'static str],
    /// Whether, and how often, it is started again when it ends.
    pub restart: Restart,
}

/// The shell components' restart policy: started again after a failure, at
/// most 3 starts in 60 seconds (what Plasma uses for plasmashell).
pub const SHELL_RESTART: Restart = Restart::OnFailure {
    burst: 3,
    interval: std::time::Duration::from_secs(60),
};

/// What a component's being ready means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ready {
    /// Once it owns this name on the session bus.
    BusName(&'static str),
    /// Once it has been started: nothing waits for it.
    Started,
}

/// The wallpaper. It uses nothing restricted.
pub const WALLPAPER: Component = Component {
    package: "deskrinch-wallpaper",
    args: &[],
    ready: Ready::Started,
    wayland_grants: &[],
    restart: SHELL_RESTART,
};

/// The notification daemon: the session's `org.freedesktop.Notifications`,
/// ready once it owns that name, so that applications started after it
/// find their notifications shown. With nothing to show it has no window.
/// It uses nothing restricted.
pub const NOTIFICATIONS: Component = Component {
    package: "deskrinch-notifications",
    args: &[],
    ready: Ready::BusName("org.freedesktop.Notifications"),
    wayland_grants: &[],
    restart: SHELL_RESTART,
};

/// What the notification daemon says, at info level, once its UI is up
/// and its display connection made: from then on it shows what comes. It
/// owns its name before that ([`NOTIFICATIONS`] is ready on the name), so a
/// starter that needs the display in use waits for this line too.
pub const NOTIFICATIONS_UP_LINE: &str = "notifications: up, showing what comes";

/// The launcher: hidden at first, it owns its name on the session bus,
/// through which a key binding (or anything else) summons it. It uses
/// nothing restricted.
pub const LAUNCHER: Component = Component {
    package: "deskrinch-launcher",
    args: &[],
    ready: Ready::BusName("org.deskrinch.Launcher"),
    wayland_grants: &[],
    restart: SHELL_RESTART,
};

/// The panel. Its taskbar follows the window list: under KWin through
/// `org_kde_plasma_window_management`, which KWin offers only to the
/// programs it grants it (the wlroots compositors' window-list protocol is
/// not restricted).
pub const PANEL: Component = Component {
    package: "deskrinch-panel",
    args: &[],
    ready: Ready::Started,
    wayland_grants: &["org_kde_plasma_window_management"],
    restart: SHELL_RESTART,
};

/// The shell's components, in the order they start.
///
/// The wallpaper comes first, so that the desktop is not bare while the
/// rest starts. The notification daemon follows, so that the session's
/// notifications have their service early. The launcher follows, hidden.
/// The panel comes last.
pub const COMPONENTS: &[Component] = &[WALLPAPER, NOTIFICATIONS, LAUNCHER, PANEL];

/// A modifier of a key binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Modifier {
    /// Control.
    Control,
    /// Alt.
    Alt,
    /// Shift.
    Shift,
    /// The logo key (Super, Meta).
    Super,
}

/// A key a binding presses, besides its modifiers.
///
/// Only the keys the shell binds are here: each compositor names a key its
/// own way, and every name a starter writes comes from a match on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// The space bar.
    Space,
}

/// A key binding: modifiers held, and a key pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    /// The modifiers held, each once.
    pub modifiers: &'static [Modifier],
    /// The key pressed.
    pub key: Key,
}

/// A command a key binding runs: a component's binary, by its package, with
/// arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    /// The Cargo package whose binary runs.
    pub package: &'static str,
    /// Its arguments.
    pub args: &'static [&'static str],
}

/// The chord that summons the launcher (shows it, or hides it if shown):
/// Control, Alt and Space. Not the logo key, which a desktop the shell runs
/// nested in takes for itself; not Alt and Space or Alt and F2, which
/// Plasma's `KRunner` takes.
pub const LAUNCHER_CHORD: Chord = Chord {
    modifiers: &[Modifier::Control, Modifier::Alt],
    key: Key::Space,
};

/// What [`LAUNCHER_CHORD`] runs.
pub const LAUNCHER_TOGGLE: Action = Action {
    package: "deskrinch-launcher",
    args: &["toggle"],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_component_is_a_distinct_package() {
        let mut packages: Vec<_> = COMPONENTS.iter().map(|c| c.package).collect();
        packages.sort_unstable();
        packages.dedup();
        assert_eq!(packages.len(), COMPONENTS.len());
    }

    #[test]
    fn each_grant_is_named_once() {
        for component in COMPONENTS {
            let mut grants = component.wayland_grants.to_vec();
            grants.sort_unstable();
            grants.dedup();
            assert_eq!(grants.len(), component.wayland_grants.len());
            assert!(grants.iter().all(|grant| !grant.is_empty()));
        }
    }

    #[test]
    fn the_launcher_chord_holds_each_modifier_once() {
        let mut modifiers = LAUNCHER_CHORD.modifiers.to_vec();
        modifiers.sort_unstable();
        modifiers.dedup();
        assert_eq!(modifiers.len(), LAUNCHER_CHORD.modifiers.len());
        assert_ne!(modifiers, []);
    }

    #[test]
    fn the_launcher_s_toggle_runs_a_component() {
        assert!(
            COMPONENTS
                .iter()
                .any(|c| c.package == LAUNCHER_TOGGLE.package)
        );
    }
}

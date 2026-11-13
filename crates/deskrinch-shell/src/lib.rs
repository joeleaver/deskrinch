//! What deskrinch's shell consists of, in one place: its components, in the
//! order they start, what each one's being ready means and what each must be
//! granted ([`COMPONENTS`]), and the shell's own key bindings
//! ([`LAUNCHER_CHORD`]).
//!
//! `cargo xtask desktop` starts the shell from this, in a nested compositor
//! in a window, and grants each component its restricted interfaces first;
//! the real login session will start it, and grant them, from this too. This
//! crate is data only: how a component is built and started, and how a
//! compositor is told about a key binding, are each starter's own.
//!
//! # Example
//!
//! ```
//! use deskrinch_shell::{COMPONENTS, Ready};
//!
//! // The launcher starts first, hidden, and is ready once it owns its name.
//! let launcher = &COMPONENTS[0];
//! assert_eq!(launcher.package, "deskrinch-launcher");
//! assert_eq!(launcher.ready, Ready::BusName("org.deskrinch.Launcher"));
//! ```

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
}

/// What a component's being ready means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ready {
    /// Once it owns this name on the session bus.
    BusName(&'static str),
    /// Once it has been started: nothing waits for it.
    Started,
}

/// The launcher: hidden at first, it owns its name on the session bus,
/// through which a key binding (or anything else) summons it. It uses
/// nothing restricted.
pub const LAUNCHER: Component = Component {
    package: "deskrinch-launcher",
    args: &[],
    ready: Ready::BusName("org.deskrinch.Launcher"),
    wayland_grants: &[],
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
};

/// The shell's components, in the order they start: the launcher, then the
/// panel.
pub const COMPONENTS: &[Component] = &[LAUNCHER, PANEL];

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
    fn the_launcher_chord_holds_each_modifier_once() {
        let mut modifiers = LAUNCHER_CHORD.modifiers.to_vec();
        modifiers.sort_unstable();
        modifiers.dedup();
        assert_eq!(modifiers.len(), LAUNCHER_CHORD.modifiers.len());
        assert_ne!(modifiers, []);
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
    fn the_launcher_s_toggle_runs_a_component() {
        assert!(
            COMPONENTS
                .iter()
                .any(|c| c.package == LAUNCHER_TOGGLE.package)
        );
    }
}

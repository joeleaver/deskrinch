//! The component's rinch [`App`], made here so that every component is made
//! alike: [`new`] for a component with a main window, [`windowless`] for
//! one whose windows its root component opens. Each returns rinch's `App`,
//! on which the component goes on as usual (`.window_props(..)`,
//! `.theme(..)`, `.run()`). Calling rinch's own constructors elsewhere is
//! refused by the workspace's clippy configuration (`disallowed-methods`).
//!
//! What they set:
//!
//! - **DevTools** ([`devtools`]): rinch answers F12 (its DevTools window)
//!   and Alt+I (inspect mode) in a `desktop` build when nothing in the app
//!   claimed the key. A shell component turns that off unless it is built
//!   with this crate's `debug` feature, which each component's own `debug`
//!   feature turns on. Cargo unifies features over one build, so when any
//!   component in a build has `debug`, every component in that build has
//!   DevTools, as with `rinch/debug` itself: `debug` is a property of the
//!   build.
//!
//!   `RINCH_DEVTOOLS=on|off` in the environment overrides this choice
//!   either way: rinch at the pinned commit resolves the setting as the
//!   environment's answer when it gives one, else the app's
//!   (`devtools_resolved`, in rinch's `crates/rinch/src/app/devtools_keys.rs`).
//!
//! # Example
//!
//! ```no_run
//! use rinch::prelude::*;
//!
//! # fn root(_scope: &mut RenderScope) -> NodeHandle { unimplemented!() }
//! // `root` is the component's root component.
//! deskrinch_component::app::new(root).run();
//! ```

use rinch::prelude::{NodeHandle, RenderScope};

/// rinch's application builder, as [`new`] and [`windowless`] return it.
pub use rinch::App;

/// Whether the component answers rinch's DevTools keys: only when built
/// with the `debug` feature.
#[must_use]
pub const fn devtools() -> bool {
    cfg!(feature = "debug")
}

/// The component's application around its root `component`, with a main
/// window (rinch's `App::new`).
#[expect(
    clippy::disallowed_methods,
    reason = "the one place the shell's components make rinch's App"
)]
pub fn new<F>(component: F) -> App<F>
where
    F: FnOnce(&mut RenderScope) -> NodeHandle + 'static,
{
    App::new(component).devtools(devtools())
}

/// The component's application with no main window: `component` opens the
/// windows (rinch's `App::windowless`).
#[expect(
    clippy::disallowed_methods,
    reason = "the one place the shell's components make rinch's App"
)]
pub fn windowless<F>(component: F) -> App<F>
where
    F: FnOnce(&mut RenderScope) -> NodeHandle + 'static,
{
    App::windowless(component).devtools(devtools())
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "debug")]
    #[test]
    fn a_debug_build_answers_the_devtools_keys() {
        assert!(super::devtools());
    }

    #[cfg(not(feature = "debug"))]
    #[test]
    fn a_build_without_debug_does_not() {
        assert!(!super::devtools());
    }
}

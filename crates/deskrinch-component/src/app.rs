//! The component's rinch [`App`], made and run here so that every component
//! is made and ends alike: [`new`] for a component with a main window,
//! [`windowless`] for one whose windows its root component opens, each
//! returning rinch's `App`, on which the component goes on as usual
//! (`.window_props(..)`, `.theme(..)`); then [`run`], whose exit status the
//! component's `main` returns. Calling rinch's own constructors, or running
//! the app elsewhere, is refused by the workspace's clippy configuration
//! (`disallowed-methods`).
//!
//! What they set:
//!
//! - **DevTools** ([`devtools`]): rinch answers F12 (its DevTools window)
//!   and Alt+I (inspect mode) when nothing in the app claimed the key. Its
//!   own default, at the pinned commit, is on in a debug build and off in a
//!   release one (`rinch::app::DEVTOOLS_DEFAULT`); a shell component sets it
//!   either way, and turns it off unless it is built with this crate's
//!   `debug` feature, which each component's own `debug` feature turns on.
//!   Cargo unifies features over one build, so when any component in a
//!   build has `debug`, every component in that build has DevTools, as with
//!   `rinch/debug` itself: `debug` is a property of the build.
//!
//!   `RINCH_DEVTOOLS=on|off` in the environment overrides this choice
//!   either way: rinch at the pinned commit resolves the setting as the
//!   environment's answer when it gives one, else the app's
//!   (`devtools_resolved`, in rinch's `crates/rinch/src/app/devtools_keys.rs`).
//!
//! How it ends ([`run`]):
//!
//! - **The app closed:** success.
//! - **Its display went away** (the compositor exited, crashed or dropped
//!   the connection): one warning saying so ([`DISPLAY_LOST_LINE`]), and
//!   [`DISPLAY_LOST`], which its starter reads as a failure to restart
//!   after.
//! - **Any other error of the event loop:** reported, and a failure.
//!
//! The lines go through the process's `tracing` subscriber
//! ([`report`](crate::report)), which rinch installs with its internal
//! errors not logged (`log_internal_errors(false)`, rinch's
//! `crates/rinch/src/logging.rs`): a write to a stderr nobody reads any more,
//! after the session has ended, is dropped rather than panicking.
//!
//! # Example
//!
//! ```no_run
//! use rinch::prelude::*;
//!
//! # fn root(_scope: &mut RenderScope) -> NodeHandle { unimplemented!() }
//! // `root` is the component's root component.
//! # fn main() -> std::process::ExitCode {
//! deskrinch_component::app::run(deskrinch_component::app::new(root))
//! # }
//! ```

use std::process::ExitCode;

use deskrinch_shell::exit_status::{DISPLAY_LOST, DISPLAY_LOST_LINE};
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

/// Runs `app` until it ends, and the status the component's `main` returns
/// (see the module's doc): success when it closed,
/// [`DISPLAY_LOST`] when its display went away, a failure otherwise.
#[expect(
    clippy::disallowed_methods,
    reason = "the one place the shell's components run rinch's App"
)]
pub fn run<F, M>(app: App<F, M>) -> ExitCode
where
    F: FnOnce(&mut RenderScope) -> NodeHandle + 'static,
{
    match app.try_run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.is_display_lost() => {
            // A warning: shown under the components' default filter.
            tracing::warn!("{DISPLAY_LOST_LINE}: {error}");
            ExitCode::from(DISPLAY_LOST)
        }
        Err(error) => {
            crate::report::error("the event loop ended", &error);
            ExitCode::FAILURE
        }
    }
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

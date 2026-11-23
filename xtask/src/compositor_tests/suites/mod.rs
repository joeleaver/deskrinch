//! The test binaries `cargo xtask compositor-tests` runs, and where: the
//! table of rows, each a [`Suite`] (`super::suite`).
//!
//! Each area's rows are in a file of their own, each row a named constant:
//! the test-support crates' ([`support`]), each library's and each
//! component's, and xtask's own. The order they run in is stated here
//! alone, in [`SUITES`], which names every row once; the table's tests,
//! and the rules over it (`passes`, `granted`, `component_rule`,
//! `ignored_rule`), read the whole of it from there.

mod launcher;
mod notifications;
mod panel;
mod support;
mod wallpaper;
mod wayland_client;
mod window_management;
mod workspaces;
mod xtask;

pub(super) use self::xtask::INPUT_CLIENT;
pub(super) use super::passes::Pass;
pub(super) use super::suite::{Grant, Output, SESSION_END, Suite};
use crate::session::Compositor;

const ALL: &[Compositor] = &[Compositor::Kwin, Compositor::Sway, Compositor::Labwc];
const KWIN: &[Compositor] = &[Compositor::Kwin];
const LABWC: &[Compositor] = &[Compositor::Labwc];
const WLROOTS: &[Compositor] = &[Compositor::Sway, Compositor::Labwc];

const PLAIN: &[Pass] = &[Pass::Plain];

/// The binaries, in the order they run.
pub(super) const SUITES: [Suite; 28] = [
    // First: every change to the session's outputs is put back exactly,
    // which the suites after it rely on.
    support::TEST_CLIENT,
    support::TEST_CLIENT_CLOSE_THEN_OPEN,
    support::TEST_CLIENT_UHD_BESIDE_FULL_HD,
    window_management::WINDOW_MANAGEMENT,
    window_management::WINDOW_MANAGEMENT_NOT_GRANTED,
    support::KWIN_VIEW,
    support::SCREEN,
    support::SCREEN_WLROOTS,
    support::SCREEN_NOT_GRANTED,
    support::INPUT,
    // The components, on what the suites above have checked.
    launcher::LAUNCHER,
    notifications::NOTIFICATIONS,
    panel::PANEL,
    panel::PANEL_LAST_OUTPUT,
    panel::PANEL_UHD_BESIDE_FULL_HD,
    wallpaper::WALLPAPER,
    workspaces::WORKSPACES_KWIN,
    workspaces::WORKSPACES_LABWC,
    workspaces::WORKSPACES_UNSUPPORTED,
    wayland_client::WAYLAND_CLIENT,
    xtask::DESKTOP,
    xtask::XTASK_INPUT,
    xtask::DISPLAY_LOST,
    xtask::ASKED_TO_STOP,
    // The suites that exercise the shared connection, again on libwayland.
    wayland_client::WAYLAND_CLIENT_ON_LIBWAYLAND,
    window_management::WINDOW_MANAGEMENT_ON_LIBWAYLAND,
    workspaces::WORKSPACES_SHARED_ON_LIBWAYLAND,
    // Last: it ends the session.
    window_management::WINDOW_MANAGEMENT_SESSION_END,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_ends_last() {
        let last = SUITES.last().unwrap();
        assert_eq!(last.filter, [SESSION_END]);
        assert!(last.compositors == ALL && last.passes == PLAIN);
    }

    /// A filter is read as test names by `--test`'s selection, so an
    /// option put there fails here, not a selection: `--skip` is its only
    /// option, and what a run is told besides has a field of its own
    /// (`output`).
    #[test]
    fn a_filter_holds_names_and_skips_only() {
        for suite in &SUITES {
            for word in suite.filter {
                assert!(
                    *word == "--skip" || !word.starts_with('-'),
                    "{:?}: {word} in its filter is not a test's name",
                    suite.target
                );
            }
        }
    }

    #[test]
    fn each_binary_is_granted_at_most_once() {
        for (index, suite) in SUITES.iter().enumerate() {
            let granted_again = SUITES[index + 1..]
                .iter()
                .any(|later| later.target == suite.target && later.grant.is_some());
            assert!(
                !(suite.grant.is_some() && granted_again),
                "{:?}",
                suite.target
            );
        }
    }
}

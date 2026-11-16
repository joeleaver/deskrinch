//! What each shell component does when its display goes away: the
//! compositor it is a client of is killed while it runs, and it ends with
//! deskrinch-shell's `DISPLAY_LOST` status, having said
//! `DISPLAY_LOST_LINE`.
//!
//! Each test runs the component in a session of its own ([`in_session`]):
//! once it has drawn its first frame (presented, or prepared for the
//! launcher started hidden), the session's compositor is killed, and the
//! component's status and what it said are read.

#[cfg(test)]
mod in_session;

#[cfg(test)]
mod lost {
    use deskrinch_shell::exit_status::{DISPLAY_LOST, DISPLAY_LOST_LINE};

    use crate::in_session::{self, Then, Until};

    #[track_caller]
    fn ends_with_display_lost(package: &str, args: &[&str], until: Until) {
        let ended = in_session::run(package, args, until, Then::KillCompositor);
        assert_eq!(
            ended.status,
            i32::from(DISPLAY_LOST),
            "{package} ended with {}; it said:\n{}",
            ended.status,
            ended.said
        );
        assert!(
            ended.said.contains(DISPLAY_LOST_LINE),
            "{package} did not say {DISPLAY_LOST_LINE:?}; it said:\n{}",
            ended.said
        );
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_wallpaper_ends_with_display_lost() {
        ends_with_display_lost("deskrinch-wallpaper", &[], Until::Presented);
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_panel_ends_with_display_lost() {
        ends_with_display_lost("deskrinch-panel", &[], Until::Presented);
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_launcher_shown_ends_with_display_lost() {
        ends_with_display_lost("deskrinch-launcher", &["--show"], Until::Presented);
    }

    /// The launcher as it nearly always is, and as it is when a compositor
    /// it was a client of crashes: hidden, its frame prepared but never
    /// presented.
    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_launcher_hidden_ends_with_display_lost() {
        ends_with_display_lost("deskrinch-launcher", &[], Until::Prepared);
    }
}

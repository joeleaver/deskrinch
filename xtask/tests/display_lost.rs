//! What each shell component does when its display goes away: the
//! compositor it is a client of is killed while it runs, and it ends with
//! deskrinch-shell's `DISPLAY_LOST` status, having said
//! `DISPLAY_LOST_LINE`.
//!
//! Each test runs the component in a session of its own ([`in_session`]),
//! granted what the shell grants it: once it has drawn its first frame
//! (presented, or prepared for the launcher started hidden), or for the
//! notification daemon once it owns its name (with no window) or shows a
//! popup sent then, the session's compositor is killed, and the
//! component's status and what it said are read.
//!
//! Up to the moment the compositor was ended, the component must have
//! logged nothing at error level: it started as it starts on the desktop.
//! What it says after is its own matter (the display-lost line is a
//! warning by design, and a part of it that had a connection of its own
//! may say that connection ended), so the cut is that moment, which
//! `cargo xtask in-session` records, not a line the component writes.

#[cfg(test)]
mod in_session;

#[cfg(test)]
mod lost {
    use deskrinch_shell::exit_status::{DISPLAY_LOST, DISPLAY_LOST_LINE};
    use deskrinch_shell::{NOTIFICATIONS, Ready};

    use crate::in_session::{self, Ended, Then, Until};

    #[track_caller]
    fn ends_with_display_lost(package: &str, args: &[&str], until: Until) -> Ended {
        let ended = in_session::run(package, args, until, Then::KillCompositor, None);
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
        assert!(
            !ended.before().contains(" ERROR "),
            "{package} logged an error before its display went away; it said then:\n{}",
            ended.before()
        );
        ended
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

    /// The notification daemon with nothing to show has no window; rinch's
    /// event loop holds the display all the same, and sees it go.
    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_notification_daemon_with_no_window_ends_with_display_lost() {
        let ended = ends_with_display_lost(
            NOTIFICATIONS.package,
            &[],
            Until::Owned(notifications_name()),
        );
        assert!(
            !ended.before().contains(" opened:"),
            "a window with nothing to show; it said then:\n{}",
            ended.before()
        );
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_notification_daemon_showing_a_popup_ends_with_display_lost() {
        let ended = ends_with_display_lost(
            NOTIFICATIONS.package,
            &[],
            Until::Notified(notifications_name()),
        );
        // Its service still stops in order: the sender is not left waiting.
        assert_eq!(
            ended.closed,
            [4],
            "the sender was not told its notification closed (reason 4); the daemon said:\n{}",
            ended.said
        );
    }

    /// The name the shell waits for the notification daemon to own.
    fn notifications_name() -> &'static str {
        match NOTIFICATIONS.ready {
            Ready::BusName(name) => name,
            Ready::Started => panic!("the shell waits for the daemon's name"),
        }
    }
}

//! What each shell component does when it is asked to stop: sent `SIGTERM`
//! (how a session or a unit stops it) or `SIGINT` once it has come as far
//! as `display_lost.rs` waits for, it ends in order with status 0, says so
//! at info level (deskrinch-shell's `asked_to_stop_line`), and logs
//! nothing at warn or error level from then on.
//!
//! The component runs with `RUST_LOG=info`: the line is at info level, and
//! a component shows warn and worse by default, so without it the line
//! would not be there to find. Each test runs the component in a session
//! of its own ([`in_session`]).

#[cfg(test)]
mod in_session;

#[cfg(test)]
mod asked {
    use deskrinch_shell::exit_status::asked_to_stop_line;

    use crate::in_session::{self, Then, Until};

    /// The level words of rinch's log format, as a line has them.
    const WARN_OR_WORSE: [&str; 2] = [" WARN ", " ERROR "];

    /// The signal a test asks the component to stop with.
    #[derive(Debug, Clone, Copy)]
    enum Stop {
        Term,
        Int,
    }

    impl Stop {
        /// What the session's side does for it.
        fn then(self) -> Then {
            match self {
                Self::Term => Then::SignalTerm,
                Self::Int => Then::SignalInt,
            }
        }

        /// Its name, as the component says it.
        fn name(self) -> &'static str {
            match self {
                Self::Term => "SIGTERM",
                Self::Int => "SIGINT",
            }
        }
    }

    #[track_caller]
    fn ends_in_order(package: &str, args: &[&str], until: Until, stop: Stop) {
        let ended = in_session::run(package, args, until, stop.then(), Some("info"));
        let said = &ended.said;
        assert_eq!(
            ended.status, 0,
            "{package} ended with {}; it said:\n{said}",
            ended.status
        );
        let line = asked_to_stop_line(stop.name());
        let Some(at) = said.find(&line) else {
            panic!("{package} did not say {line:?}; it said:\n{said}");
        };
        assert!(
            at >= ended.before().len(),
            "{package} said {line:?} before it was sent the signal; it said:\n{said}"
        );
        assert!(
            !said.contains(" ERROR "),
            "{package} logged an error; it said:\n{said}"
        );
        let after = &said[at..];
        assert!(
            !WARN_OR_WORSE.iter().any(|level| after.contains(level)),
            "{package} warned while it stopped; it said from the stop on:\n{after}"
        );
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_wallpaper_ends_in_order_on_sigterm() {
        ends_in_order("deskrinch-wallpaper", &[], Until::Presented, Stop::Term);
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_panel_ends_in_order_on_sigterm() {
        ends_in_order("deskrinch-panel", &[], Until::Presented, Stop::Term);
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_panel_ends_in_order_on_sigint() {
        ends_in_order("deskrinch-panel", &[], Until::Presented, Stop::Int);
    }

    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_launcher_shown_ends_in_order_on_sigterm() {
        ends_in_order(
            "deskrinch-launcher",
            &["--show"],
            Until::Presented,
            Stop::Term,
        );
    }

    /// The launcher hidden: a component with no window shown, which must
    /// end as promptly as one with a window.
    #[test]
    #[ignore = "needs a compositor: cargo xtask compositor-tests"]
    fn the_launcher_hidden_ends_in_order_on_sigterm() {
        ends_in_order("deskrinch-launcher", &[], Until::Prepared, Stop::Term);
    }
}

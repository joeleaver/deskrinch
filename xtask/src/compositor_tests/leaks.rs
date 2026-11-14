//! Whether a suite left the session's outputs as it found them.
//!
//! A test that changes the session's outputs and does not put them back
//! passes itself, and a later, unrelated suite in the same session fails
//! for it. So the runner lists the outputs at the session's start and
//! again after each suite's binary, and a difference fails the session
//! there, naming the suite that left it; so does a listing that cannot be
//! made after a suite.

use std::fmt;

use deskrinch_test_client::{OutputState, layout_differences};

/// What to say about the session's outputs after the suite `suite`,
/// listed at its start as `start` and now as `now`; `None` when it left
/// them as they were. A listing that could not be made fails the session
/// there too: the suite most likely took the compositor down or left it
/// hung.
pub(super) fn after_suite<E: fmt::Display>(
    suite: &str,
    start: &[OutputState],
    now: Result<Vec<OutputState>, E>,
) -> Option<String> {
    match now {
        Ok(now) => layout_differences(start, &now).map(|differences| {
            format!(
                "{suite} left the session's outputs changed: at the session's start, \
                 {differences}"
            )
        }),
        Err(error) => Some(format!(
            "{suite}: the session's outputs could not be listed afterwards: {error}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use deskrinch_test_client::Mode;
    use deskrinch_test_wayland::Transform;

    use super::*;

    fn output(name: &str, x: i32) -> OutputState {
        OutputState {
            name: name.to_owned(),
            enabled: true,
            position: (x, 0),
            mode: Some(Mode {
                width: 1280,
                height: 720,
                refresh_mhz: 0,
            }),
            scale: 1.0,
            transform: Transform::Normal,
        }
    }

    #[test]
    fn one_row_ends_the_session_and_is_not_followed_by_a_listing() {
        let ending: Vec<String> = super::super::suites::SUITES
            .iter()
            .filter(|suite| suite.ends_the_session())
            .map(|suite| suite.target.name())
            .collect();
        assert_eq!(ending, ["deskrinch-window-management compositor"]);
    }

    #[test]
    fn a_suite_that_left_the_outputs_as_they_were_says_nothing() {
        let start = [output("HEADLESS-2", 0), output("HEADLESS-1", 1280)];
        let now: Result<_, String> = Ok(start.to_vec());
        assert_eq!(after_suite("x compositor", &start, now), None);
    }

    #[test]
    fn a_suite_that_moved_an_output_is_named_with_both_listings() {
        // As sway left it after an output was turned on again without its
        // position.
        let start = [output("HEADLESS-2", 0), output("HEADLESS-1", 1280)];
        let now = [output("HEADLESS-2", 0), output("HEADLESS-1", 0)];
        assert_eq!(
            after_suite::<String>("deskrinch-wallpaper compositor", &start, Ok(now.to_vec()))
                .unwrap(),
            "deskrinch-wallpaper compositor left the session's outputs changed: at the \
             session's start, the outputs were\n\
             \x20 HEADLESS-2 at 0,0, 1280x720@0mHz, scale 1, Normal\n\
             \x20 HEADLESS-1 at 1280,0, 1280x720@0mHz, scale 1, Normal\n\
             and are now\n\
             \x20 HEADLESS-2 at 0,0, 1280x720@0mHz, scale 1, Normal\n\
             \x20 HEADLESS-1 at 0,0, 1280x720@0mHz, scale 1, Normal"
        );
    }

    #[test]
    fn a_listing_that_cannot_be_made_fails_the_session_at_that_suite() {
        let start = [output("HEADLESS-2", 0), output("HEADLESS-1", 1280)];
        let now: Result<Vec<OutputState>, &str> =
            Err("could not connect to /tmp/deskrinch-session-x/run/wayland-1");
        assert_eq!(
            after_suite("deskrinch-panel compositor", &start, now).unwrap(),
            "deskrinch-panel compositor: the session's outputs could not be listed \
             afterwards: could not connect to /tmp/deskrinch-session-x/run/wayland-1"
        );
    }
}

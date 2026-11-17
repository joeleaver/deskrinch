//! The column filled one popup at a time, each popup waited for in the
//! daemon's own lines rather than on the screen.
//!
//! A wait on the screen ([`Notifications::settled`]) captures every output
//! until two captures a moment apart match: over a second each time. A
//! column filled that way paid for one or two such waits per popup, and so
//! grew with the output's height. The daemon says each fit it settles on
//! (`showing N, M waiting`), and presents exactly one frame that draws for
//! each change, after the layout the fit was read from: so its fit for the
//! notifications live is settled once its last fit line counts them all and
//! a drawing frame has come after it. A test that fills the column waits for
//! that after each popup, and on the screen once, for the state it is about.

use std::time::Duration;

use deskrinch_test_component::drew;
use deskrinch_test_notify::Signal;
use deskrinch_test_wait::wait_for;

use super::daemon::fit_of;
use super::{Column, Notifications};

/// How long the daemon's output must stay unchanged for a fit to count as
/// settled: many frames' worth, against a frame of the same change still to
/// come.
const QUIET_LINES: Duration = Duration::from_millis(300);

/// The reason `NotificationClosed` carries for a close by its sender.
const CLOSED: u32 = 3;

/// The fit the daemon's `lines` settle on for `live` notifications: its last
/// fit line, when it counts `live` (shown and waiting) and a frame that drew
/// came after it.
fn settled_fit(lines: &[String], live: usize) -> Option<(usize, usize)> {
    let at = lines.iter().rposition(|line| fit_of(line).is_some())?;
    let (shown, waiting) = fit_of(&lines[at])?;
    let drawn = lines[at..].iter().any(|line| drew(line));
    (shown + waiting == live && drawn).then_some((shown, waiting))
}

impl Notifications {
    /// Waits, in the daemon's lines, until it has settled on a fit of
    /// `live` notifications and drawn it, with nothing more said for
    /// [`QUIET_LINES`]; the fit then.
    pub(crate) fn fit_drawn(&self, live: usize) -> (usize, usize) {
        wait_for(&format!("a fit of {live} drawn"), || {
            let lines = self.daemon().watch().lines();
            let Some(fit) = settled_fit(&lines, live) else {
                return Err(self.said());
            };
            std::thread::sleep(QUIET_LINES);
            if self.daemon().watch().lines().len() == lines.len() {
                Ok(fit)
            } else {
                Err("it said more".to_owned())
            }
        })
    }

    /// Waits until the daemon has said nothing for [`QUIET_LINES`]: what a
    /// change set going (its frame, its fit lines) has come.
    pub(crate) fn quiet(&self) {
        wait_for("the daemon quiet", || {
            let said = self.daemon().watch().lines().len();
            std::thread::sleep(QUIET_LINES);
            if self.daemon().watch().lines().len() == said {
                Ok(())
            } else {
                Err("it said more")
            }
        });
    }

    /// Sends one-line popups ("Filling") until one waits, each waited for in
    /// the daemon's lines ([`fit_drawn`](Self::fit_drawn)); the live ones,
    /// oldest first, and the fit then.
    pub(crate) fn fill(&mut self) -> (Vec<u32>, (usize, usize)) {
        let mut live = Vec::new();
        loop {
            live.push(self.client.simple(0, "Filling", "", 0));
            let fit = self.fit_drawn(live.len());
            if fit.1 > 0 {
                return (live, fit);
            }
        }
    }

    /// Fills the column with no room to spare for another popup: popups
    /// until one waits, then the newest closed, every one shown; waited for
    /// on the screen once, at the end. The live ones, oldest first, and the
    /// column then.
    pub(crate) fn full_column(&mut self) -> (Vec<u32>, Column) {
        let (mut live, _) = self.fill();
        let newest = live.pop().unwrap();
        self.client.close(newest).unwrap();
        assert_eq!(self.next_signal(), Signal::Closed(newest, CLOSED));
        assert_eq!(self.fit_drawn(live.len()), (live.len(), 0));
        let column = self.settled(live.len(), 0);
        (live, column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|line| (*line).to_owned()).collect()
    }

    const FRAME: &str = "[PERF] window 1 frame 3: style 0.10ms | paint_frames=1";

    #[test]
    fn a_fit_is_settled_once_it_counts_every_popup_and_a_frame_drew_after_it() {
        let drawn = lines(&[
            "INFO column: showing 6, 1 waiting",
            "INFO column: showing 5, 2 waiting",
            FRAME,
        ]);
        assert_eq!(settled_fit(&drawn, 7), Some((5, 2)));
        // Another count: a popup the daemon has not taken yet.
        assert_eq!(settled_fit(&drawn, 8), None);
        // No frame after the last fit: the fit may move again.
        let undrawn = lines(&[FRAME, "INFO column: showing 6, 1 waiting"]);
        assert_eq!(settled_fit(&undrawn, 7), None);
        // A frame of cached pixels draws nothing.
        let cached = lines(&[
            "INFO column: showing 6, 1 waiting",
            "[PERF] window 1 frame 4: style 0.10ms | paint_cached_frames=1",
        ]);
        assert_eq!(settled_fit(&cached, 7), None);
        assert_eq!(settled_fit(&lines(&[]), 0), None);
    }
}

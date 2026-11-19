//! Waiting for the screen to keep still, where nothing reports that a
//! change has ended: a compositor's animation of a window opening or
//! closing, or a client's frames still coming. Two captures alike, a gap
//! apart, are the only sign of it, and the gap is the compositor's
//! ([`still_gap_for`]).

use std::fmt::Debug;
use std::thread;
use std::time::Duration;

use deskrinch_test_client::Compositor;
use deskrinch_test_wait::{Deadline, PATIENCE, wait_for};

use crate::error::CaptureError;
use crate::image::Image;
use crate::screen::Screen;

/// How long the screen must not change to count as still under the
/// compositor the tests run in ([`still_gap_for`]).
///
/// # Panics
///
/// Outside a nested session ([`Compositor::current`]).
#[must_use]
pub fn still_gap() -> Duration {
    still_gap_for(Compositor::current())
}

/// How long the screen must not change to count as still under
/// `compositor`: longer than any animation it runs by default, so that two
/// captures alike this far apart cannot both fall inside one.
///
/// - KWin: 600 ms, its longest, the fading popups' close
///   (`animationTime(150) * 4` at the default speed a fresh session has; its
///   scale effect takes 200 ms each way). Read and shown on KWin 6.6.6.
/// - sway and labwc animate nothing: a few frames' time is enough (six at
///   60 Hz).
#[must_use]
pub const fn still_gap_for(compositor: Compositor) -> Duration {
    match compositor {
        Compositor::Kwin => Duration::from_millis(600),
        Compositor::Sway | Compositor::Labwc => Duration::from_millis(100),
    }
}

/// Outputs' pictures, each with its output's name, in the order the
/// compositor announced the outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pictures(pub Vec<(String, Image)>);

impl Pictures {
    /// Every output of `screen` captured now, or with `only` that output's
    /// alone.
    ///
    /// # Errors
    ///
    /// When a capture fails ([`Screen::capture`]).
    pub fn take(screen: &mut Screen, only: Option<&str>) -> Result<Self, CaptureError> {
        let mut pictures = Vec::new();
        for name in screen.outputs() {
            if only.is_none_or(|only| only == name) {
                let image = screen.capture(&name)?;
                pictures.push((name, image));
            }
        }
        Ok(Self(pictures))
    }

    /// Every output of `screen` once it is still: the same in two captures
    /// [`still_gap`] apart.
    ///
    /// # Panics
    ///
    /// When a capture fails, or the screen has not kept still within
    /// [`PATIENCE`]; outside a nested session.
    #[must_use]
    #[track_caller]
    pub fn still(screen: &mut Screen) -> Self {
        Self::still_only(screen, None, still_gap())
    }

    /// The output named `output` alone, once it is still, as
    /// [`still`](Self::still) waits for every output: cheaper for a wait
    /// that knows where to look.
    ///
    /// # Panics
    ///
    /// As [`still`](Self::still), and when there is no such output.
    #[must_use]
    #[track_caller]
    pub fn still_of(screen: &mut Screen, output: &str) -> Self {
        Self::still_only(screen, Some(output), still_gap())
    }

    #[track_caller]
    fn still_only(screen: &mut Screen, only: Option<&str>, gap: Duration) -> Self {
        let deadline = Deadline::patience();
        let take = |screen: &mut Screen| {
            Self::take(screen, only).unwrap_or_else(|error| panic!("capturing the screen: {error}"))
        };
        let mut last = take(screen);
        loop {
            thread::sleep(gap);
            let now = take(screen);
            if now == last {
                return now;
            }
            assert!(
                !deadline.passed(),
                "waited {PATIENCE:?} for the screen to keep still ({gap:?} without a change)"
            );
            last = now;
        }
    }

    /// What `judge` takes from the output named `output` once it is still
    /// ([`still_of`](Self::still_of)), the output looked at again until it
    /// is still and `judge` answers with a value; `what` names the wait.
    ///
    /// The judgment for a picture that stays as it is. Whether a change
    /// comes about is shown by the first capture that has it
    /// ([`Screen::capture_until`]): no earlier picture can have it. That
    /// something stays is judged on a still screen, because the first
    /// capture after the event that could change it can come before the
    /// change would reach the screen, and passes whatever the program
    /// does. With a capture of a few milliseconds this is the common case,
    /// not the rare one: the wallpaper's check that a picture replaced by
    /// one that cannot be shown stays let a mutant that shows the colour
    /// through on 11 of 27 passes as a first capture, and on none as this.
    ///
    /// # Panics
    ///
    /// When no such picture comes within [`PATIENCE`], with `what` and
    /// `judge`'s last answer; as [`still_of`](Self::still_of).
    #[track_caller]
    pub fn judged<T, S: Debug>(
        screen: &mut Screen,
        output: &str,
        what: &str,
        judge: impl FnMut(&Image) -> Result<T, S>,
    ) -> T {
        Self::judged_over(screen, output, still_gap(), what, judge)
    }

    /// [`judged`](Self::judged), still meaning the same in two captures
    /// `gap` apart rather than [`still_gap`]'s.
    ///
    /// The default gap is the one to use: it is calibrated per compositor
    /// to outlast what the compositor itself animates. A longer gap is
    /// right only where the change judged takes longer than the default
    /// gap to reach the screen after the event the test waits for (a
    /// client told to change its picture some time later, as this crate's
    /// own test of the judgment does), never to make a wait pass.
    ///
    /// # Panics
    ///
    /// As [`judged`](Self::judged).
    #[track_caller]
    pub fn judged_over<T, S: Debug>(
        screen: &mut Screen,
        output: &str,
        gap: Duration,
        what: &str,
        mut judge: impl FnMut(&Image) -> Result<T, S>,
    ) -> T {
        wait_for(what, || {
            let still = Self::still_only(screen, Some(output), gap);
            let image = still
                .get(output)
                .unwrap_or_else(|| panic!("{what}: no output {output}"));
            judge(image)
        })
    }

    /// The picture of the output named `name`, if there is one.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Image> {
        self.0
            .iter()
            .find(|(output, _)| output == name)
            .map(|(_, image)| image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rgba;

    #[test]
    fn kwin_waits_out_its_longest_animation_and_wlroots_a_few_frames() {
        assert_eq!(still_gap_for(Compositor::Kwin), Duration::from_millis(600));
        assert_eq!(still_gap_for(Compositor::Sway), Duration::from_millis(100));
        assert_eq!(still_gap_for(Compositor::Labwc), Duration::from_millis(100));
    }

    #[test]
    fn a_picture_is_found_by_its_output_s_name() {
        let image = |level| Image::new(1, 1, vec![Rgba::opaque(level, level, level)]).unwrap();
        let pictures = Pictures(vec![("A-1".into(), image(1)), ("B-2".into(), image(2))]);
        assert_eq!(pictures.get("B-2"), Some(&image(2)));
        assert_eq!(pictures.get("C-3"), None);
    }
}

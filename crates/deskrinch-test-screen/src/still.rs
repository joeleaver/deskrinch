//! Waiting for the screen to keep still, where nothing reports that a
//! change has ended: a compositor's animation of a window opening or
//! closing, or a client's frames still coming. Two captures alike, a gap
//! apart, are the only sign of it, and the gap is the compositor's
//! ([`still_gap_for`]).

use std::thread;
use std::time::Duration;

use deskrinch_test_client::Compositor;
use deskrinch_test_wait::{Deadline, PATIENCE};

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
        Self::still_only(screen, None)
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
        Self::still_only(screen, Some(output))
    }

    #[track_caller]
    fn still_only(screen: &mut Screen, only: Option<&str>) -> Self {
        let deadline = Deadline::patience();
        let gap = still_gap();
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

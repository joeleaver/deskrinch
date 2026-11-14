//! A nested session's outputs as a whole, and changing them from a test so
//! that they are put back exactly as they were.
//!
//! Every change states every output in full: whether it is on, and for one
//! that is on its position, mode, scale and transform. Nothing is left to
//! the compositor, because compositors fill a property left out in their
//! own ways: sway puts an output turned on again at (0,0), on top of
//! another, and KWin moves the second output to (0,0) when the first goes
//! and leaves it there when the first comes back. A change returns a
//! [`RestoreOutputs`], which holds the whole layout from before (not just
//! the output changed, for KWin's sake) and puts it back when restored or
//! dropped, so that a test that fails does not leave the session's later
//! tests a different layout.

use std::fmt;

use deskrinch_test_wayland::Transform;

use crate::outputs as wlroots;
use deskrinch_test_wait::{Deadline, PATIENCE, POLL};

use crate::{Compositor, Error, OutputChange, kscreen};

/// An output's mode: its size in physical pixels and its refresh rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mode {
    /// The width, in physical pixels.
    pub width: u32,
    /// The height, in physical pixels.
    pub height: u32,
    /// The refresh rate, in mHz (0 when the compositor has none, as a
    /// headless output may).
    pub refresh_mhz: u32,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}@{}mHz", self.width, self.height, self.refresh_mhz)
    }
}

/// An output as the compositor reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputState {
    /// Its name (`HEADLESS-1`, `Virtual-0`).
    pub name: String,
    /// Whether it is on. An output that is off keeps the rest as it was
    /// last reported, which the compositor may not say.
    pub enabled: bool,
    /// The position of its top-left corner in the layout, in logical
    /// pixels.
    pub position: (i32, i32),
    /// Its current mode; `None` when the compositor reports none (an
    /// output that is off may not say). An output to be turned on needs
    /// one: a change refuses it without ([`Error::NoMode`]).
    pub mode: Option<Mode>,
    /// Its scale: how many physical pixels a logical one covers.
    pub scale: f64,
    /// Its transform.
    pub transform: Transform,
}

impl OutputState {
    /// This state with `change` made.
    #[must_use]
    pub fn changed(&self, change: OutputChange) -> Self {
        Self {
            scale: change.scale.unwrap_or(self.scale),
            transform: change.transform.unwrap_or(self.transform),
            ..self.clone()
        }
    }

    /// Whether `other` is the same output in the same state, comparing
    /// only what is reported for an output that is off: whether it is.
    fn matches(&self, other: &Self) -> bool {
        self.name == other.name
            && self.enabled == other.enabled
            && (!self.enabled
                || (self.position == other.position
                    && self.mode == other.mode
                    && (self.scale - other.scale).abs() < 1e-6
                    && self.transform == other.transform))
    }
}

impl fmt::Display for OutputState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.enabled {
            return write!(f, "{} off", self.name);
        }
        let mode = self
            .mode
            .map_or_else(|| "no mode".to_owned(), |mode| mode.to_string());
        write!(
            f,
            "{} at {},{}, {mode}, scale {}, {:?}",
            self.name, self.position.0, self.position.1, self.scale, self.transform
        )
    }
}

/// The mode of `output`, to be on: refused with [`Error::NoMode`] when the
/// compositor reported none, before anything is asked of it.
pub(crate) fn mode_to_set(output: &OutputState) -> Result<Mode, Error> {
    output
        .mode
        .ok_or_else(|| Error::NoMode(output.name.clone()))
}

/// What differs between two layouts, `before` and `after`, for a
/// message; `None` when they are the same (each output, by name, in the
/// same state).
#[must_use]
pub fn layout_differences(before: &[OutputState], after: &[OutputState]) -> Option<String> {
    let same = before.len() == after.len()
        && before.iter().all(|output| {
            after
                .iter()
                .any(|other| other.name == output.name && output.matches(other))
        });
    if same {
        return None;
    }
    let list = |layout: &[OutputState]| {
        layout
            .iter()
            .map(|output| format!("  {output}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Some(format!(
        "the outputs were\n{}\nand are now\n{}",
        list(before),
        list(after)
    ))
}

/// The nested compositor's outputs, as it reports them: through
/// `wlr-output-management` under sway and labwc, and `kscreen-doctor` under
/// KWin.
///
/// # Errors
///
/// When the compositor cannot be asked, or does not answer within
/// [`PATIENCE`](deskrinch_test_wait::PATIENCE).
///
/// # Panics
///
/// Outside a nested session, as [`Compositor::current`].
pub fn outputs() -> Result<Vec<OutputState>, Error> {
    match Compositor::current() {
        Compositor::Sway | Compositor::Labwc => wlroots::layout_of_env(),
        Compositor::Kwin => kscreen::layout(),
    }
}

/// Makes the nested compositor's outputs `layout`, stating each in full.
fn apply(layout: &[OutputState]) -> Result<(), Error> {
    match Compositor::current() {
        Compositor::Sway | Compositor::Labwc => wlroots::apply_to_env(layout),
        Compositor::Kwin => kscreen::apply_layout(layout),
    }
}

/// Makes the outputs `layout` and waits until the compositor reports what
/// `reached` takes, saying what still differs from `layout` if it does not
/// within [`PATIENCE`](deskrinch_test_wait::PATIENCE).
fn apply_and_wait(
    layout: &[OutputState],
    reached: impl Fn(&[OutputState]) -> bool,
) -> Result<(), Error> {
    apply(layout)?;
    let deadline = Deadline::patience();
    loop {
        let now = outputs()?;
        if reached(&now) {
            return Ok(());
        }
        if deadline.passed() {
            return Err(Error::LayoutNotReached {
                differences: layout_differences(layout, &now)
                    .unwrap_or_else(|| "the outputs are as asked, but not as waited for".into()),
                patience: PATIENCE,
                applications: 1,
            });
        }
        std::thread::sleep(POLL);
    }
}

/// Whether the output named `name` in `layout` is as `wanted` says, by
/// what a change sets: on or off and, when on, its scale and transform.
/// The others' positions are the compositor's to change meanwhile: KWin
/// moves the second output to (0,0) while the first is off, and labwc lays
/// outputs out side by side again when one turns.
fn shows(layout: &[OutputState], name: &str, wanted: &OutputState) -> bool {
    layout.iter().any(|output| {
        output.name == name
            && output.enabled == wanted.enabled
            && (!wanted.enabled
                || ((output.scale - wanted.scale).abs() < 1e-6
                    && output.transform == wanted.transform))
    })
}

/// Makes the output named `name` what `change` makes of it, the others
/// stated as they are, and returns the guard that puts `before` back once
/// the compositor reports the change.
fn change_with(
    name: &str,
    change: impl Fn(&OutputState) -> OutputState,
) -> Result<RestoreOutputs, Error> {
    let before = outputs()?;
    let target = with(&before, name, change)?;
    let wanted = target
        .iter()
        .find(|output| output.name == name)
        .cloned()
        .ok_or_else(|| Error::UnknownOutput(name.to_owned()))?;
    let guard = RestoreOutputs::new(before);
    apply_and_wait(&target, |now| shows(now, name, &wanted))?;
    Ok(guard)
}

/// Puts the outputs back as `before`, applying it again while the
/// compositor reports otherwise ([`converge`](crate::converge)).
fn restore(before: &[OutputState]) -> Result<(), Error> {
    crate::converge::converge(before, apply, outputs, crate::converge::SETTLE, PATIENCE).map(drop)
}

/// The layout `before` with the output `name` replaced by what `change`
/// makes of it.
fn with(
    before: &[OutputState],
    name: &str,
    change: impl Fn(&OutputState) -> OutputState,
) -> Result<Vec<OutputState>, Error> {
    if !before.iter().any(|output| output.name == name) {
        return Err(Error::UnknownOutput(name.to_owned()));
    }
    Ok(before
        .iter()
        .map(|output| {
            if output.name == name {
                change(output)
            } else {
                output.clone()
            }
        })
        .collect())
}

/// Turns the output named `name` of the nested compositor off, every other
/// output stated as it is, and returns once the compositor reports it off,
/// with the guard that puts the whole layout back. The compositor may move
/// the others meanwhile (KWin moves the second output to (0,0) while the
/// first is off).
///
/// # Errors
///
/// When there is no output so named, the compositor refuses, or does not
/// report the change within [`PATIENCE`](deskrinch_test_wait::PATIENCE).
///
/// # Panics
///
/// Outside a nested session, as [`Compositor::current`].
pub fn turn_output_off(name: &str) -> Result<RestoreOutputs, Error> {
    change_with(name, |output| OutputState {
        enabled: false,
        ..output.clone()
    })
}

/// Changes the scale or transform of the output named `name` of the nested
/// compositor, every other output stated as it is, and returns once the
/// compositor reports the change, with the guard that puts the whole layout
/// back. The compositor may move the others meanwhile (labwc lays outputs
/// out side by side again when one turns).
///
/// # Errors
///
/// As [`turn_output_off`].
///
/// # Panics
///
/// Outside a nested session, as [`Compositor::current`].
pub fn change_output(name: &str, change: OutputChange) -> Result<RestoreOutputs, Error> {
    change_with(name, |output| output.changed(change))
}

/// The session's outputs as they were before a change, put back by
/// [`restore`](Self::restore), or when dropped.
///
/// Dropped without being restored, it puts them back too. If that fails
/// while the thread is not already panicking, it panics, so that the test
/// that could not put the outputs back fails; while it is panicking, it
/// prints the failure (a second panic would abort the test binary, and the
/// other tests' results with it).
#[derive(Debug)]
#[must_use = "dropping it at once puts the outputs back at once"]
pub struct RestoreOutputs {
    before: Vec<OutputState>,
    restored: bool,
}

impl RestoreOutputs {
    fn new(before: Vec<OutputState>) -> Self {
        Self {
            before,
            restored: false,
        }
    }

    /// The layout it puts back.
    #[must_use]
    pub fn layout(&self) -> &[OutputState] {
        &self.before
    }

    /// Puts the outputs back as they were, every one stated in full, and
    /// returns once the compositor reports them so.
    ///
    /// # Errors
    ///
    /// When the compositor refuses, or does not report the layout within
    /// [`PATIENCE`](deskrinch_test_wait::PATIENCE).
    pub fn restore(mut self) -> Result<(), Error> {
        self.restored = true;
        restore(&self.before)
    }
}

impl Drop for RestoreOutputs {
    fn drop(&mut self) {
        if self.restored {
            return;
        }
        if let Err(error) = restore(&self.before) {
            if std::thread::panicking() {
                eprintln!("could not put the session's outputs back: {error}");
            } else {
                panic!("could not put the session's outputs back: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(name: &str, x: i32) -> OutputState {
        OutputState {
            name: name.to_owned(),
            enabled: true,
            position: (x, 0),
            mode: Some(Mode {
                width: 1280,
                height: 720,
                refresh_mhz: 60_000,
            }),
            scale: 1.0,
            transform: Transform::Normal,
        }
    }

    #[test]
    fn the_same_layout_in_another_order_has_no_differences() {
        let before = [output("A", 0), output("B", 1280)];
        let after = [output("B", 1280), output("A", 0)];
        assert_eq!(layout_differences(&before, &after), None);
    }

    #[test]
    fn an_output_moved_is_named_with_both_layouts() {
        let before = [output("A", 0), output("B", 1280)];
        let after = [output("A", 0), output("B", 0)];
        let message = layout_differences(&before, &after).unwrap();
        assert_eq!(
            message,
            "the outputs were\n\
             \x20 A at 0,0, 1280x720@60000mHz, scale 1, Normal\n\
             \x20 B at 1280,0, 1280x720@60000mHz, scale 1, Normal\n\
             and are now\n\
             \x20 A at 0,0, 1280x720@60000mHz, scale 1, Normal\n\
             \x20 B at 0,0, 1280x720@60000mHz, scale 1, Normal"
        );
    }

    #[test]
    fn every_property_and_the_outputs_themselves_count() {
        let before = [output("A", 0), output("B", 1280)];
        let changes: [&dyn Fn(&mut OutputState); 5] = [
            &|o| o.enabled = false,
            &|o| o.mode.as_mut().unwrap().refresh_mhz = 59_940,
            &|o| o.scale = 2.0,
            &|o| o.transform = Transform::Rotated90,
            &|o| o.name = "C".into(),
        ];
        for change in changes {
            let mut after = before.clone();
            change(&mut after[1]);
            assert!(layout_differences(&before, &after).is_some(), "{after:?}");
        }
        assert!(layout_differences(&before, &before[..1]).is_some());
    }

    #[test]
    fn an_output_off_is_compared_only_by_being_off() {
        let mut off = output("B", 1280);
        off.enabled = false;
        let mut moved = off.clone();
        moved.position = (0, 0);
        assert_eq!(layout_differences(&[off], &[moved]), None);
    }

    #[test]
    fn a_change_keeps_what_it_does_not_name() {
        let before = output("A", 0);
        let turned = before.changed(OutputChange {
            scale: None,
            transform: Some(Transform::Rotated90),
        });
        assert_eq!(turned.transform, Transform::Rotated90);
        assert_eq!(turned.position, before.position);
        assert_eq!(turned.mode, before.mode);
        assert!((turned.scale - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_change_is_seen_on_its_output_whatever_the_others_do() {
        let wanted = OutputState {
            transform: Transform::Rotated90,
            ..output("B", 1280)
        };
        // labwc lays B out again after A; KWin moves it to (0,0).
        let moved = [
            output("A", 0),
            OutputState {
                position: (720, 0),
                ..wanted.clone()
            },
        ];
        assert!(shows(&moved, "B", &wanted));
        assert!(!shows(&[output("A", 0), output("B", 1280)], "B", &wanted));
        let off = OutputState {
            enabled: false,
            ..output("A", 0)
        };
        assert!(shows(&[off.clone(), output("B", 0)], "A", &off));
        assert!(!shows(&[output("A", 0)], "A", &off));
        assert!(!shows(&[output("A", 0)], "C", &off));
    }

    #[test]
    fn no_mode_is_no_mode_compared_and_refused_to_set() {
        let none = OutputState {
            mode: None,
            ..output("A", 0)
        };
        let alone = std::slice::from_ref(&none);
        assert!(layout_differences(&[output("A", 0)], alone).is_some());
        assert_eq!(layout_differences(alone, alone), None);
        assert_eq!(none.to_string(), "A at 0,0, no mode, scale 1, Normal");
        assert!(matches!(mode_to_set(&none), Err(Error::NoMode(name)) if name == "A"));
        assert!(mode_to_set(&output("A", 0)).is_ok());
    }

    #[test]
    fn a_change_to_an_unknown_output_is_refused() {
        let layout = [output("A", 0)];
        assert!(matches!(
            with(&layout, "B", Clone::clone),
            Err(Error::UnknownOutput(name)) if name == "B"
        ));
        let off = with(&layout, "A", |o| OutputState {
            enabled: false,
            ..o.clone()
        })
        .unwrap();
        assert!(!off[0].enabled);
    }
}

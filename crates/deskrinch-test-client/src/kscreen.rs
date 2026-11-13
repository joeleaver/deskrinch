//! KWin's outputs, changed through KDE's `kscreen-doctor` (`libkscreen-bin`
//! on Debian and Ubuntu): KWin's own output protocol is not usable from Rust
//! here (see [`configure_output`](crate::configure_output)).
//!
//! The settings are written here, once: a test changes an output with
//! [`change_output`](crate::change_output) or
//! [`set_output_enabled`](crate::set_output_enabled), and a session that
//! sets its outputs up before its first client runs `kscreen-doctor` with
//! [`kscreen_settings`] itself, as one of its own clients.

use std::process::Command;

use deskrinch_test_wayland::Transform;

use crate::{Error, OutputChange};

/// The program.
pub const KSCREEN_DOCTOR: &str = "kscreen-doctor";

/// `kscreen-doctor`'s arguments that apply `change` to the output named
/// `name`: none for a change that changes nothing.
#[must_use]
pub fn kscreen_settings(name: &str, change: OutputChange) -> Vec<String> {
    let scale = change
        .scale
        // `2` rather than `2.0`, and `1.5` as it is.
        .map(|scale| format!("output.{name}.scale.{scale}"));
    let rotation = change
        .transform
        .map(|transform| format!("output.{name}.rotation.{}", rotation(transform)));
    scale.into_iter().chain(rotation).collect()
}

/// `kscreen-doctor`'s argument that turns the output named `name` on or off.
pub(crate) fn enabled_setting(name: &str, enabled: bool) -> String {
    let state = if enabled { "enable" } else { "disable" };
    format!("output.{name}.{state}")
}

/// `transform`'s name for `kscreen-doctor`'s `rotation` (`KScreen` calls a
/// quarter turn counter-clockwise "left").
fn rotation(transform: Transform) -> &'static str {
    match transform {
        Transform::Normal => "none",
        Transform::Rotated90 => "left",
        Transform::Rotated180 => "inverted",
        Transform::Rotated270 => "right",
        Transform::Flipped => "flipped",
        Transform::Flipped90 => "flipped90",
        Transform::Flipped180 => "flipped180",
        Transform::Flipped270 => "flipped270",
    }
}

/// Runs `kscreen-doctor` with `settings` for the output named `name`, as a
/// client of the session the test runs in, and returns once it has applied
/// them.
pub(crate) fn apply(name: &str, settings: &[String]) -> Result<(), Error> {
    let output = Command::new(KSCREEN_DOCTOR)
        // Qt 5 picks X11 by default, and there is no X server in a nested
        // session; Qt 6 finds the session's Wayland on its own.
        .env("QT_QPA_PLATFORM", "wayland")
        .args(settings)
        .output()
        .map_err(|source| Error::Spawn {
            program: KSCREEN_DOCTOR,
            source,
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::OutputRefused {
            name: name.to_owned(),
            reason: format!(
                "{KSCREEN_DOCTOR} {}: {}",
                output.status,
                String::from_utf8_lossy(&[output.stdout, output.stderr].concat())
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_name_the_output_scale_and_rotation() {
        let change = OutputChange {
            scale: Some(1.5),
            transform: Some(Transform::Rotated90),
        };
        assert_eq!(
            kscreen_settings("Virtual-1", change),
            [
                "output.Virtual-1.scale.1.5",
                "output.Virtual-1.rotation.left"
            ]
        );
        let whole = OutputChange {
            scale: Some(2.0),
            transform: None,
        };
        assert_eq!(kscreen_settings("V", whole), ["output.V.scale.2"]);
        assert_eq!(
            kscreen_settings("V", OutputChange::default()),
            Vec::<String>::new()
        );
        assert_eq!(enabled_setting("V", false), "output.V.disable");
        assert_eq!(enabled_setting("V", true), "output.V.enable");
    }

    #[test]
    fn a_quarter_turn_counter_clockwise_is_left() {
        assert_eq!(rotation(Transform::Rotated90), "left");
        assert_eq!(rotation(Transform::Rotated270), "right");
        assert_eq!(rotation(Transform::Flipped90), "flipped90");
    }
}

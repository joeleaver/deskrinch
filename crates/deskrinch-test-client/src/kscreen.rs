//! KWin's outputs, read and set through KDE's `kscreen-doctor`
//! (`libkscreen-bin` on Debian and Ubuntu): KWin's own output protocol is
//! not usable from Rust here (see [`crate::outputs`]).
//!
//! A layout is read from `kscreen-doctor -j` ([`kscreen_outputs`]) and set
//! in one call that states every output: enabled or disabled, and for one
//! that is on its position, mode, scale and rotation. KWin moves the second
//! output to (0,0) when the first is turned off and does not move it back,
//! so turning an output on alone would not restore a layout.
//!
//! A session that sets its outputs up before its first client runs
//! `kscreen-doctor` with [`kscreen_settings`] itself, as one of its own
//! clients.

use std::process::Command;

use deskrinch_test_wayland::Transform;
use serde::Deserialize;

use crate::layout::{Mode, OutputState};
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

/// The transform `KScreen`'s `rotation` flag stands for, as `-j` writes it
/// (`KScreen::Output::Rotation`).
fn transform_of(flag: u32) -> Option<Transform> {
    Some(match flag {
        1 => Transform::Normal,
        2 => Transform::Rotated90,
        4 => Transform::Rotated180,
        8 => Transform::Rotated270,
        16 => Transform::Flipped,
        32 => Transform::Flipped90,
        64 => Transform::Flipped180,
        128 => Transform::Flipped270,
        _ => return None,
    })
}

/// `kscreen-doctor -j`, the part read here.
#[derive(Debug, Deserialize)]
struct Listing {
    outputs: Vec<Listed>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    name: String,
    enabled: bool,
    pos: Point,
    scale: f64,
    rotation: u32,
    current_mode_id: Option<String>,
    #[serde(default)]
    modes: Vec<ListedMode>,
}

#[derive(Debug, Deserialize)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedMode {
    id: String,
    refresh_rate: f64,
    size: Size,
}

#[derive(Debug, Deserialize)]
struct Size {
    width: u32,
    height: u32,
}

impl ListedMode {
    fn mode(&self) -> Mode {
        Mode {
            width: self.size.width,
            height: self.size.height,
            // Hz as `KScreen` gives it, to mHz.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a refresh rate is a small positive number of Hz"
            )]
            refresh_mhz: (self.refresh_rate * 1000.0).round() as u32,
        }
    }
}

/// The outputs `json`, the output of `kscreen-doctor -j`, describes.
///
/// # Errors
///
/// [`Error::Kscreen`] when it is not such a listing, or names a rotation
/// `KScreen` does not have.
pub fn kscreen_outputs(json: &str) -> Result<Vec<OutputState>, Error> {
    let listing: Listing = serde_json::from_str(json).map_err(|error| Error::Kscreen {
        why: format!("not a listing of outputs: {error}"),
    })?;
    listing
        .outputs
        .into_iter()
        .map(|listed| {
            let transform = transform_of(listed.rotation).ok_or_else(|| Error::Kscreen {
                why: format!(
                    "{} has an unknown rotation {}",
                    listed.name, listed.rotation
                ),
            })?;
            let mode = listed
                .modes
                .iter()
                .find(|mode| Some(&mode.id) == listed.current_mode_id.as_ref())
                .map_or(
                    Mode {
                        width: 0,
                        height: 0,
                        refresh_mhz: 0,
                    },
                    ListedMode::mode,
                );
            Ok(OutputState {
                name: listed.name,
                enabled: listed.enabled,
                position: (listed.pos.x, listed.pos.y),
                mode,
                scale: listed.scale,
                transform,
            })
        })
        .collect()
}

/// `kscreen-doctor`'s arguments that make the outputs `layout`, every
/// output stated: its mode by the id `ids` gives it (the output's name and
/// mode to `KScreen`'s mode id), as `KScreen` matches a mode name by its own
/// spelling of the refresh rate.
fn layout_settings(
    layout: &[OutputState],
    ids: impl Fn(&str, Mode) -> Option<String>,
) -> Result<Vec<String>, Error> {
    let mut settings = Vec::new();
    for output in layout {
        let name = &output.name;
        if !output.enabled {
            settings.push(format!("output.{name}.disable"));
            continue;
        }
        let id = ids(name, output.mode).ok_or_else(|| Error::Kscreen {
            why: format!("{name} has no mode {}", output.mode),
        })?;
        let (x, y) = output.position;
        settings.extend([
            format!("output.{name}.enable"),
            format!("output.{name}.position.{x},{y}"),
            format!("output.{name}.mode.{id}"),
            format!("output.{name}.scale.{}", output.scale),
            format!("output.{name}.rotation.{}", rotation(output.transform)),
        ]);
    }
    Ok(settings)
}

/// Runs `kscreen-doctor` with `args`, as a client of the session the test
/// runs in, and returns its standard output once it has done.
fn run(args: &[String]) -> Result<String, Error> {
    let output = Command::new(KSCREEN_DOCTOR)
        // Qt 5 picks X11 by default, and there is no X server in a nested
        // session; Qt 6 finds the session's Wayland on its own.
        .env("QT_QPA_PLATFORM", "wayland")
        .args(args)
        .output()
        .map_err(|source| Error::Spawn {
            program: KSCREEN_DOCTOR,
            source,
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(Error::OutputRefused {
            name: "the outputs".to_owned(),
            reason: format!(
                "{KSCREEN_DOCTOR} {}: {}",
                output.status,
                String::from_utf8_lossy(&[output.stdout, output.stderr].concat())
            ),
        })
    }
}

/// The session's outputs, as KWin reports them.
pub(crate) fn layout() -> Result<Vec<OutputState>, Error> {
    kscreen_outputs(&run(&["-j".to_owned()])?)
}

/// Makes the session's outputs `layout`, in one call.
pub(crate) fn apply_layout(layout: &[OutputState]) -> Result<(), Error> {
    let listing: Listing =
        serde_json::from_str(&run(&["-j".to_owned()])?).map_err(|error| Error::Kscreen {
            why: format!("not a listing of outputs: {error}"),
        })?;
    let ids = |name: &str, mode: Mode| {
        listing
            .outputs
            .iter()
            .find(|output| output.name == name)?
            .modes
            .iter()
            .find(|listed| listed.mode() == mode)
            .map(|listed| listed.id.clone())
    };
    run(&layout_settings(layout, ids)?).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As KWin 6.6's `kscreen-doctor -j` lists two virtual outputs, cut to
    /// what is read.
    const LISTING: &str = r#"{"outputs":[
        {"name":"Virtual-0","enabled":true,"pos":{"x":0,"y":0},"scale":1,"rotation":1,
         "currentModeId":"1","modes":[{"id":"1","name":"1280x720@60","refreshRate":60,
         "size":{"height":720,"width":1280}}],"type":0,"id":1},
        {"name":"Virtual-1","enabled":false,"pos":{"x":1280,"y":0},"scale":1.5,"rotation":2,
         "currentModeId":"2","modes":[{"id":"2","name":"1280x720@59.94","refreshRate":59.94,
         "size":{"height":720,"width":1280}}],"type":0,"id":2}]}"#;

    #[test]
    fn reads_the_listing() {
        let outputs = kscreen_outputs(LISTING).unwrap();
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].name, "Virtual-0");
        assert!(outputs[0].enabled);
        assert_eq!(outputs[0].position, (0, 0));
        assert_eq!(
            outputs[0].mode,
            Mode {
                width: 1280,
                height: 720,
                refresh_mhz: 60_000
            }
        );
        assert!(!outputs[1].enabled);
        assert_eq!(outputs[1].position, (1280, 0));
        assert_eq!(outputs[1].mode.refresh_mhz, 59_940);
        assert!((outputs[1].scale - 1.5).abs() < f64::EPSILON);
        assert_eq!(outputs[1].transform, Transform::Rotated90);
        assert!(kscreen_outputs("{}").is_err());
        let rotated = LISTING.replace("\"rotation\":2", "\"rotation\":3");
        assert!(kscreen_outputs(&rotated).is_err());
    }

    #[test]
    fn a_layout_states_every_output_in_full() {
        let mut outputs = kscreen_outputs(LISTING).unwrap();
        outputs[1].enabled = true;
        let ids = |name: &str, _| Some(if name == "Virtual-0" { "1" } else { "2" }.to_owned());
        assert_eq!(
            layout_settings(&outputs, ids).unwrap(),
            [
                "output.Virtual-0.enable",
                "output.Virtual-0.position.0,0",
                "output.Virtual-0.mode.1",
                "output.Virtual-0.scale.1",
                "output.Virtual-0.rotation.none",
                "output.Virtual-1.enable",
                "output.Virtual-1.position.1280,0",
                "output.Virtual-1.mode.2",
                "output.Virtual-1.scale.1.5",
                "output.Virtual-1.rotation.left",
            ]
        );
        outputs[0].enabled = false;
        let settings = layout_settings(&outputs, ids).unwrap();
        assert_eq!(settings[0], "output.Virtual-0.disable");
        assert!(layout_settings(&outputs, |_, _| None).is_err());
    }

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
    }

    #[test]
    fn rotations_go_both_ways() {
        assert_eq!(rotation(Transform::Rotated90), "left");
        assert_eq!(rotation(Transform::Rotated270), "right");
        assert_eq!(rotation(Transform::Flipped90), "flipped90");
        for (flag, transform) in [(1, Transform::Normal), (8, Transform::Rotated270)] {
            assert_eq!(transform_of(flag), Some(transform));
        }
        assert_eq!(transform_of(0), None);
    }
}

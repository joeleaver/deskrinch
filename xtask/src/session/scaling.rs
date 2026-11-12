//! Putting a session's outputs at the scale and transform asked for, before
//! its first client, and checking that they are.

use std::ffi::OsStr;

use deskrinch_test_client::{OutputChange, OutputInfo, TestClient, configure_every_output};

use super::{Outputs, Session};
use crate::error::Error;

/// How a compositor's outputs get their scale and transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scaling {
    /// From its start-up configuration (sway's `output * scale … transform
    /// …`).
    StartUp,
    /// Set by the session through `wlr-output-management` once it runs
    /// (labwc, which has no start-up setting for a headless output's).
    OutputManagement,
    /// Set by the session with KDE's `kscreen-doctor` once it runs (KWin:
    /// its `--scale` enlarges the virtual outputs' mode instead, it has no
    /// start-up transform, and its own output protocol is not usable from
    /// Rust here).
    KscreenDoctor,
}

/// KDE's command-line tool for output settings.
const KSCREEN_DOCTOR: &str = "kscreen-doctor";

impl Session {
    /// Puts every output at `outputs`' scale and transform, where the
    /// compositor did not start with them, then checks what every output
    /// reports.
    pub(super) fn set_up_outputs(&self, outputs: Outputs) -> Result<(), Error> {
        if !outputs.is_plain() {
            match self.compositor.scaling() {
                Scaling::StartUp => {}
                Scaling::OutputManagement => self.set_up_through_output_management(outputs)?,
                Scaling::KscreenDoctor => self.set_up_through_kscreen_doctor(outputs)?,
            }
        }
        self.check_outputs(outputs)
    }

    /// The path of the compositor's Wayland socket.
    fn socket_path(&self) -> std::path::PathBuf {
        self.root.join("run").join(&self.socket)
    }

    /// Sets every output through `wlr-output-management`, the way a test's
    /// `configure_output` does, and returns once it applies.
    fn set_up_through_output_management(&self, outputs: Outputs) -> Result<(), Error> {
        let change = OutputChange {
            scale: Some(outputs.scale.get()),
            transform: Some(outputs.transform.get()),
        };
        configure_every_output(&self.socket_path(), change).map_err(|source| Error::OutputSetup {
            compositor: self.compositor.name(),
            what: format!("set up {outputs}"),
            source: Box::new(source),
            log: self.log(),
        })
    }

    /// Sets every output with `kscreen-doctor`, run as a client of the
    /// session like any other (its environment, its process group).
    fn set_up_through_kscreen_doctor(&self, outputs: Outputs) -> Result<(), Error> {
        let names = self.connect_client()?.output_names();
        let settings = names.iter().flat_map(|name| {
            [
                format!("output.{name}.scale.{}", outputs.scale),
                format!(
                    "output.{name}.rotation.{}",
                    outputs.transform.kscreen_name()
                ),
            ]
        });
        let output = self
            .client(OsStr::new(KSCREEN_DOCTOR))
            // Qt 5 picks X11 by default, and there is no X server here;
            // Qt 6 finds the session's Wayland on its own.
            .env("QT_QPA_PLATFORM", "wayland")
            .args(settings)
            .output()
            .map_err(|source| Error::Spawn {
                program: KSCREEN_DOCTOR.into(),
                source,
            })?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Error::ScaleToolFailed {
                program: KSCREEN_DOCTOR,
                status: output.status,
                output: String::from_utf8_lossy(&[output.stdout, output.stderr].concat())
                    .into_owned(),
                log: self.log(),
            })
        }
    }

    /// Checks that every output is as `outputs` asks: its transform, and its
    /// scale. The whole scale `wl_output` reports (rounded up) cannot tell
    /// 1.5 from 2, so its logical size (`xdg-output`) must also be its mode
    /// divided by the scale, turned with the transform. A compositor that
    /// ignored or rounded a setting fails the session instead of running
    /// otherwise.
    fn check_outputs(&self, outputs: Outputs) -> Result<(), Error> {
        let client = self.connect_client()?;
        let wrong: Vec<String> = client
            .output_names()
            .into_iter()
            .filter_map(|name| {
                let info = client.output_info(&name).unwrap_or_default();
                mismatch(outputs, info).map(|why| format!("{name} {why}"))
            })
            .collect();
        if wrong.is_empty() {
            Ok(())
        } else {
            Err(Error::WrongOutputs {
                compositor: self.compositor.name(),
                asked: outputs.to_string(),
                outputs: wrong.join("; "),
                log: self.log(),
            })
        }
    }

    /// A Wayland client of the session, for setting it up.
    fn connect_client(&self) -> Result<TestClient, Error> {
        TestClient::connect_to(&self.socket_path()).map_err(|source| Error::OutputSetup {
            compositor: self.compositor.name(),
            what: "list the outputs".into(),
            source: Box::new(source),
            log: self.log(),
        })
    }
}

/// Why an output reporting `info` is not as `outputs` asks, if it is not.
fn mismatch(outputs: Outputs, info: OutputInfo) -> Option<String> {
    let (scale, transform) = (outputs.scale, outputs.transform);
    if info.transform != transform.get() {
        return Some(format!(
            "reports transform {}, not {transform}",
            super::OutputTransform::from(info.transform)
        ));
    }
    if info.scale != scale.whole() {
        return Some(format!(
            "reports whole scale {}, not {}",
            info.scale,
            scale.whole()
        ));
    }
    let (Some(mode), Some(logical)) = (info.mode, info.logical_size) else {
        return Some("reports no mode or no logical size".into());
    };
    let expected = if transform.get().swaps_axes() {
        (mode.1, mode.0)
    } else {
        mode
    };
    // Compositors round the logical size either way: within one pixel.
    let fits = |physical: i32, logical: i32| {
        (f64::from(physical) / scale.get() - f64::from(logical)).abs() < 1.0
    };
    (!(fits(expected.0, logical.0) && fits(expected.1, logical.1))).then(|| {
        format!(
            "is {}x{} logical for a {}x{} mode, not that mode at {scale}, transform {transform}",
            logical.0, logical.1, mode.0, mode.1
        )
    })
}

#[cfg(test)]
mod tests {
    use deskrinch_test_wayland::Transform;

    use super::*;

    fn info(scale: i32, logical: (i32, i32)) -> OutputInfo {
        OutputInfo {
            scale,
            mode: Some((1280, 720)),
            logical_size: Some(logical),
            logical_position: Some((0, 0)),
            transform: Transform::Normal,
        }
    }

    fn outputs(scale: &str, transform: &str) -> Outputs {
        Outputs {
            count: 1,
            scale: scale.parse().unwrap(),
            transform: transform.parse().unwrap(),
        }
    }

    #[test]
    fn a_fractional_scale_is_told_from_the_whole_one_above() {
        let at = |scale| outputs(scale, "normal");
        assert_eq!(mismatch(at("1.5"), info(2, (853, 480))), None);
        assert_eq!(mismatch(at("1.5"), info(2, (854, 480))), None);
        let rounded = mismatch(at("1.5"), info(2, (640, 360))).unwrap();
        assert!(rounded.contains("640x360"), "{rounded}");
        assert_eq!(mismatch(at("2"), info(2, (640, 360))), None);
        assert!(mismatch(at("2"), info(2, (853, 480))).is_some());
    }

    #[test]
    fn the_whole_scale_must_match_too() {
        let ignored = mismatch(outputs("2", "normal"), info(1, (1280, 720))).unwrap();
        assert!(ignored.contains("whole scale 1"), "{ignored}");
        assert_eq!(mismatch(outputs("1", "normal"), info(1, (1280, 720))), None);
    }

    #[test]
    fn a_turned_output_must_report_its_transform_and_turned_size() {
        let turned = OutputInfo {
            transform: Transform::Rotated90,
            ..info(2, (360, 640))
        };
        assert_eq!(mismatch(outputs("2", "90"), turned), None);
        let unturned = mismatch(outputs("2", "90"), info(2, (640, 360))).unwrap();
        assert!(unturned.contains("transform normal, not 90"), "{unturned}");
        let wrong_size = OutputInfo {
            logical_size: Some((640, 360)),
            ..turned
        };
        assert!(mismatch(outputs("2", "90"), wrong_size).is_some());
        // Upright, a turned logical size is wrong.
        assert!(mismatch(outputs("2", "normal"), info(2, (360, 640))).is_some());
    }

    #[test]
    fn a_silent_output_does_not_fit() {
        let silent = OutputInfo {
            logical_size: None,
            ..info(2, (0, 0))
        };
        assert!(mismatch(outputs("2", "normal"), silent).is_some());
    }
}

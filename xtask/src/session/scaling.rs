//! Putting a session's outputs at the scale asked for, before its first
//! client, and checking that they are.

use std::ffi::OsStr;

use deskrinch_test_client::{OutputChange, OutputInfo, TestClient, configure_every_output};

use super::{Scale, Session};
use crate::error::Error;

/// How a compositor's outputs get their scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scaling {
    /// From its start-up configuration (sway's `output * scale`).
    StartUp,
    /// Set by the session through `wlr-output-management` once it runs
    /// (labwc, which has no start-up setting for a headless output's scale).
    OutputManagement,
    /// Set by the session with KDE's `kscreen-doctor` once it runs (KWin:
    /// its `--scale` enlarges the virtual outputs' mode instead, and its
    /// own output protocol is not usable from Rust here).
    KscreenDoctor,
}

/// KDE's command-line tool for output settings.
const KSCREEN_DOCTOR: &str = "kscreen-doctor";

impl Session {
    /// Puts every output at `scale`, where the compositor did not start at
    /// it, then checks the scale every output reports.
    pub(super) fn scale_outputs(&self, scale: Scale) -> Result<(), Error> {
        if !scale.is_one() {
            match self.compositor.scaling() {
                Scaling::StartUp => {}
                Scaling::OutputManagement => self.scale_through_output_management(scale)?,
                Scaling::KscreenDoctor => self.scale_through_kscreen_doctor(scale)?,
            }
        }
        self.check_scale(scale)
    }

    /// The path of the compositor's Wayland socket.
    fn socket_path(&self) -> std::path::PathBuf {
        self.root.join("run").join(&self.socket)
    }

    /// Sets every output to `scale` through `wlr-output-management`, the way
    /// a test's `configure_output` does, and returns once it applies.
    fn scale_through_output_management(&self, scale: Scale) -> Result<(), Error> {
        let change = OutputChange {
            scale: Some(scale.get()),
            transform: None,
        };
        configure_every_output(&self.socket_path(), change).map_err(|source| Error::OutputSetup {
            compositor: self.compositor.name(),
            what: format!("set the outputs' scale to {scale}"),
            source: Box::new(source),
            log: self.log(),
        })
    }

    /// Sets every output to `scale` with `kscreen-doctor`, run as a client
    /// of the session like any other (its environment, its process group).
    fn scale_through_kscreen_doctor(&self, scale: Scale) -> Result<(), Error> {
        let outputs = self.connect_client()?.output_names();
        let output = self
            .client(OsStr::new(KSCREEN_DOCTOR))
            // Qt 5 picks X11 by default, and there is no X server here;
            // Qt 6 finds the session's Wayland on its own.
            .env("QT_QPA_PLATFORM", "wayland")
            .args(
                outputs
                    .iter()
                    .map(|name| format!("output.{name}.scale.{scale}")),
            )
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

    /// Checks that every output is at `scale`: the whole scale `wl_output`
    /// reports (rounded up) cannot tell 1.5 from 2, so its logical size
    /// (`xdg-output`) must also be its mode divided by `scale`. A compositor
    /// that ignored or rounded the setting fails the session instead of
    /// running at another scale.
    fn check_scale(&self, scale: Scale) -> Result<(), Error> {
        let client = self.connect_client()?;
        let wrong: Vec<String> = client
            .output_names()
            .into_iter()
            .filter_map(|name| {
                let info = client.output_info(&name).unwrap_or_default();
                mismatch(scale, info).map(|why| format!("{name} {why}"))
            })
            .collect();
        if wrong.is_empty() {
            Ok(())
        } else {
            Err(Error::WrongScale {
                compositor: self.compositor.name(),
                asked: scale.to_string(),
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

/// Why an output reporting `info` is not at `scale`, if it is not.
fn mismatch(scale: Scale, info: OutputInfo) -> Option<String> {
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
    // Compositors round the logical size either way: within one pixel.
    let fits = |physical: i32, logical: i32| {
        (f64::from(physical) / scale.get() - f64::from(logical)).abs() < 1.0
    };
    let upright = fits(mode.0, logical.0) && fits(mode.1, logical.1);
    let turned = fits(mode.0, logical.1) && fits(mode.1, logical.0);
    (!upright && !turned).then(|| {
        format!(
            "is {}x{} logical for a {}x{} mode, not that mode at {scale}",
            logical.0, logical.1, mode.0, mode.1
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(scale: i32, logical: (i32, i32)) -> OutputInfo {
        OutputInfo {
            scale,
            mode: Some((1280, 720)),
            logical_size: Some(logical),
        }
    }

    fn scale(text: &str) -> Scale {
        text.parse().unwrap()
    }

    #[test]
    fn a_fractional_scale_is_told_from_the_whole_one_above() {
        assert_eq!(mismatch(scale("1.5"), info(2, (853, 480))), None);
        assert_eq!(mismatch(scale("1.5"), info(2, (854, 480))), None);
        let rounded = mismatch(scale("1.5"), info(2, (640, 360))).unwrap();
        assert!(rounded.contains("640x360"), "{rounded}");
        assert_eq!(mismatch(scale("2"), info(2, (640, 360))), None);
        assert!(mismatch(scale("2"), info(2, (853, 480))).is_some());
    }

    #[test]
    fn the_whole_scale_must_match_too() {
        let ignored = mismatch(scale("2"), info(1, (1280, 720))).unwrap();
        assert!(ignored.contains("whole scale 1"), "{ignored}");
        assert_eq!(mismatch(Scale::ONE, info(1, (1280, 720))), None);
    }

    #[test]
    fn a_turned_output_fits_and_a_silent_one_does_not() {
        assert_eq!(mismatch(scale("2"), info(2, (360, 640))), None);
        let silent = OutputInfo {
            logical_size: None,
            ..info(2, (0, 0))
        };
        assert!(mismatch(scale("2"), silent).is_some());
    }
}

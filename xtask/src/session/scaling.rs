//! Putting a session's outputs at the scale and transform asked for, before
//! its first client, and checking that they are.

use std::ffi::OsStr;

use deskrinch_test_client::{
    KSCREEN_DOCTOR, Mode, OutputChange, OutputInfo, OutputState, TestClient,
    configure_every_output, kscreen_add_mode, kscreen_select_mode, kscreen_settings,
    set_wlroots_outputs_at, wlroots_outputs_at,
};

use super::{Doing, OutputSize, Outputs, Session};
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

impl Session {
    /// Puts every output at `outputs`' scale and transform, where the
    /// compositor did not start with them, then checks what every output
    /// reports. A failure from a compositor that has exited meanwhile is
    /// reported as its exit ([`Session::unless_exited`]).
    pub(super) fn set_up_outputs(&mut self, outputs: Outputs) -> Result<(), Error> {
        let set_up = self.set_up_and_check(outputs);
        set_up.map_err(|error| self.unless_exited(Doing::SettingUpOutputs, error))
    }

    fn set_up_and_check(&self, outputs: Outputs) -> Result<(), Error> {
        if !outputs.is_plain() {
            match self.compositor.scaling() {
                Scaling::StartUp => {}
                Scaling::OutputManagement if outputs.has_sizes() => {
                    self.lay_out_through_output_management(outputs)?;
                }
                Scaling::OutputManagement => self.set_up_through_output_management(outputs)?,
                Scaling::KscreenDoctor => self.set_up_through_kscreen_doctor(outputs)?,
            }
        }
        self.check_outputs(outputs)
    }

    /// The path of the compositor's Wayland socket.
    pub(crate) fn socket_path(&self) -> std::path::PathBuf {
        self.root.join("run").join(&self.socket)
    }

    /// Sets every output through `wlr-output-management`, the way a test's
    /// `configure_output` does, and returns once it applies.
    fn set_up_through_output_management(&self, outputs: Outputs) -> Result<(), Error> {
        configure_every_output(&self.socket_path(), change_of(outputs))
            .map_err(|source| self.setup_failed(outputs, source))
    }

    /// Lays every output out through `wlr-output-management` as `outputs`
    /// asks, its mode (a custom one, which a headless output takes), its
    /// place side by side, its scale and transform, and returns once it
    /// applies.
    fn lay_out_through_output_management(&self, outputs: Outputs) -> Result<(), Error> {
        let socket = self.socket_path();
        let current =
            wlroots_outputs_at(&socket).map_err(|source| self.setup_failed(outputs, source))?;
        let names = self.connect_client()?.output_names();
        let layout: Vec<OutputState> = names
            .iter()
            .zip(outputs.layout())
            .filter_map(|(name, (size, position))| {
                let output = current.iter().find(|output| &output.name == name)?;
                Some(OutputState {
                    enabled: true,
                    position,
                    mode: Some(Mode {
                        width: size.width,
                        height: size.height,
                        // The compositor's choice, as a headless output has.
                        refresh_mhz: 0,
                    }),
                    scale: outputs.scale.get(),
                    transform: outputs.transform.get(),
                    ..output.clone()
                })
            })
            .collect();
        set_wlroots_outputs_at(&socket, &layout)
            .map_err(|source| self.setup_failed(outputs, source))
    }

    fn setup_failed(&self, outputs: Outputs, source: deskrinch_test_client::Error) -> Error {
        Error::OutputSetup {
            compositor: self.compositor.name(),
            what: format!("set up {outputs}"),
            source: Box::new(source),
            log: self.log(),
        }
    }

    /// Sets every output with `kscreen-doctor`, run as a client of the
    /// session like any other (its environment, its process group): first
    /// each size asked for that an output does not list (KWin's virtual
    /// outputs list only the mode they started with), added as a custom
    /// mode; then each output's mode and place, where sizes were asked
    /// for, and its scale and transform.
    fn set_up_through_kscreen_doctor(&self, outputs: Outputs) -> Result<(), Error> {
        let names = self.connect_client()?.output_names();
        let sized: Vec<(&String, Mode, (i32, i32))> = if outputs.has_sizes() {
            names
                .iter()
                .zip(outputs.layout())
                .map(|(name, (size, position))| (name, kscreen_mode(size), position))
                .collect()
        } else {
            Vec::new()
        };
        let added: Vec<String> = sized
            .iter()
            .filter(|(_, mode, _)| *mode != kscreen_mode(OutputSize::DEFAULT))
            .map(|(name, mode, _)| kscreen_add_mode(name, *mode))
            .collect();
        if !added.is_empty() {
            self.kscreen_doctor(added)?;
        }
        let change = change_of(outputs);
        let mut settings: Vec<String> = sized
            .iter()
            .flat_map(|(name, mode, position)| {
                let placed = OutputChange {
                    position: Some(*position),
                    ..OutputChange::default()
                };
                kscreen_settings(name, placed)
                    .into_iter()
                    .chain([kscreen_select_mode(name, *mode)])
            })
            .collect();
        settings.extend(names.iter().flat_map(|name| kscreen_settings(name, change)));
        self.kscreen_doctor(settings)
    }

    /// Runs `kscreen-doctor` with `settings` as a client of the session.
    fn kscreen_doctor(&self, settings: Vec<String>) -> Result<(), Error> {
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
            .enumerate()
            .filter_map(|(index, name)| {
                let info = client.output_info(&name).unwrap_or_default();
                mismatch(outputs, outputs.size(index), info).map(|why| format!("{name} {why}"))
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
    /// The session's outputs' names, in the order the compositor announced
    /// them: the order the session made them.
    pub(crate) fn output_names(&self) -> Result<Vec<String>, Error> {
        Ok(self.connect_client()?.output_names())
    }

    fn connect_client(&self) -> Result<TestClient, Error> {
        TestClient::connect_to(&self.socket_path()).map_err(|source| Error::OutputSetup {
            compositor: self.compositor.name(),
            what: "list the outputs".into(),
            source: Box::new(source),
            log: self.log(),
        })
    }
}

/// The change of scale and transform `outputs` asks of each output.
fn change_of(outputs: Outputs) -> OutputChange {
    OutputChange {
        scale: Some(outputs.scale.get()),
        transform: Some(outputs.transform.get()),
        position: None,
    }
}

/// `size` as `KScreen` gives a mode: at 60 Hz, as KWin's virtual outputs
/// run.
fn kscreen_mode(size: OutputSize) -> Mode {
    Mode {
        width: size.width,
        height: size.height,
        refresh_mhz: 60_000,
    }
}

/// Why an output reporting `info` is not as `outputs` asks, if it is not,
/// `size` being the mode asked of it.
fn mismatch(outputs: Outputs, size: OutputSize, info: OutputInfo) -> Option<String> {
    let (scale, transform) = (outputs.scale, outputs.transform);
    let asked = (
        i32::try_from(size.width).unwrap_or(i32::MAX),
        i32::try_from(size.height).unwrap_or(i32::MAX),
    );
    if outputs.has_sizes()
        && let Some(mode) = info.mode
        && mode != asked
    {
        return Some(format!("has a {}x{} mode, not {size}", mode.0, mode.1));
    }
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
        Outputs::new(1, scale.parse().unwrap(), transform.parse().unwrap())
    }

    /// An output asked for a size has a mode of that size; without one,
    /// the mode is not judged (an output in a window takes the window's).
    #[test]
    fn an_output_asked_for_a_size_must_have_it() {
        let size: OutputSize = "1920x1080".parse().unwrap();
        let sized = Outputs::plain(1).sized(&[size]).unwrap();
        let reported = |mode: (i32, i32)| OutputInfo {
            mode: Some(mode),
            logical_size: Some(mode),
            ..info(1, mode)
        };
        assert_eq!(mismatch(sized, size, reported((1920, 1080))), None);
        assert_eq!(
            mismatch(sized, size, reported((1280, 720))).as_deref(),
            Some("has a 1280x720 mode, not 1920x1080")
        );
        let plain = outputs("1", "normal");
        assert_eq!(
            mismatch(plain, OutputSize::DEFAULT, reported((1920, 1080))),
            None
        );
    }

    #[test]
    fn a_fractional_scale_is_told_from_the_whole_one_above() {
        let at = |scale| outputs(scale, "normal");
        assert_eq!(
            mismatch(at("1.5"), OutputSize::DEFAULT, info(2, (853, 480))),
            None
        );
        assert_eq!(
            mismatch(at("1.5"), OutputSize::DEFAULT, info(2, (854, 480))),
            None
        );
        let rounded = mismatch(at("1.5"), OutputSize::DEFAULT, info(2, (640, 360))).unwrap();
        assert!(rounded.contains("640x360"), "{rounded}");
        assert_eq!(
            mismatch(at("2"), OutputSize::DEFAULT, info(2, (640, 360))),
            None
        );
        assert!(mismatch(at("2"), OutputSize::DEFAULT, info(2, (853, 480))).is_some());
    }

    #[test]
    fn the_whole_scale_must_match_too() {
        let ignored = mismatch(
            outputs("2", "normal"),
            OutputSize::DEFAULT,
            info(1, (1280, 720)),
        )
        .unwrap();
        assert!(ignored.contains("whole scale 1"), "{ignored}");
        assert_eq!(
            mismatch(
                outputs("1", "normal"),
                OutputSize::DEFAULT,
                info(1, (1280, 720))
            ),
            None
        );
    }

    #[test]
    fn a_turned_output_must_report_its_transform_and_turned_size() {
        let turned = OutputInfo {
            transform: Transform::Rotated90,
            ..info(2, (360, 640))
        };
        assert_eq!(
            mismatch(outputs("2", "90"), OutputSize::DEFAULT, turned),
            None
        );
        let unturned =
            mismatch(outputs("2", "90"), OutputSize::DEFAULT, info(2, (640, 360))).unwrap();
        assert!(unturned.contains("transform normal, not 90"), "{unturned}");
        let wrong_size = OutputInfo {
            logical_size: Some((640, 360)),
            ..turned
        };
        assert!(mismatch(outputs("2", "90"), OutputSize::DEFAULT, wrong_size).is_some());
        // Upright, a turned logical size is wrong.
        assert!(
            mismatch(
                outputs("2", "normal"),
                OutputSize::DEFAULT,
                info(2, (360, 640))
            )
            .is_some()
        );
    }

    #[test]
    fn a_silent_output_does_not_fit() {
        let silent = OutputInfo {
            logical_size: None,
            ..info(2, (0, 0))
        };
        assert!(mismatch(outputs("2", "normal"), OutputSize::DEFAULT, silent).is_some());
    }
}

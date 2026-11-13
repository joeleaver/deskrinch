//! Turning a nested session's output off and on from a test: its
//! `wl_output` global goes, and comes back.

use std::process::Command;

use crate::{Compositor, Error, outputs};

/// KDE's tool for KWin's outputs (`libkscreen-bin`).
const KSCREEN_DOCTOR: &str = "kscreen-doctor";

/// Turns the output named `name` of the nested compositor on or off, and
/// returns once the compositor has done it: through `wlr-output-management`
/// under sway and labwc, and through KDE's `kscreen-doctor` under KWin.
/// Clients learn of it a moment later, through the registry (the output's
/// `wl_output` global goes, or comes back).
///
/// # Errors
///
/// As [`configure_output`](crate::configure_output) under sway and labwc;
/// under KWin, when `kscreen-doctor` cannot be run or fails.
///
/// # Panics
///
/// Outside a nested session, as [`Compositor::current`].
pub fn set_output_enabled(name: &str, enabled: bool) -> Result<(), Error> {
    match Compositor::current() {
        Compositor::Sway | Compositor::Labwc => outputs::set_enabled(name, enabled),
        Compositor::Kwin => {
            let state = if enabled { "enable" } else { "disable" };
            let output = Command::new(KSCREEN_DOCTOR)
                // Qt 5 picks X11 by default, and there is no X server in a
                // nested session; Qt 6 finds the session's Wayland on its own.
                .env("QT_QPA_PLATFORM", "wayland")
                .arg(format!("output.{name}.{state}"))
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
    }
}

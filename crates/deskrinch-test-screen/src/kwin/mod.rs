//! KWin's `org.kde.KWin.ScreenShot2` on the session bus: for KWin, which
//! offers neither Wayland capture protocol.
//!
//! `CaptureScreen` takes the output's name, options and a descriptor to
//! write to, answers with the image's description (size, stride, a `QImage`
//! format), then writes the raw pixels and closes the descriptor. The
//! descriptor is a memory file's ([`sink`] says why not a pipe's). KWin
//! answers only callers it has granted the interface to; see the
//! `deskrinch-kwin-access` crate.

mod sink;

use std::collections::HashMap;
use std::io;
use std::time::Duration;

use deskrinch_bus::Bus;
use deskrinch_test_wayland::Deadline;
use zbus::{Connection, Fd, OwnedVariant, Variant};

use self::sink::Sink;

use crate::Transform;
use crate::buffer::{Buffer, Layout};
use crate::error::{CaptureError, ConnectError};
use crate::image::Image;

/// KWin's screenshot service, its object and its interface: also the name
/// KWin's grant lists.
pub(crate) const INTERFACE: &str = "org.kde.KWin.ScreenShot2";
const PATH: &str = "/org/kde/KWin/ScreenShot2";

/// A connection to the session bus with KWin's screenshot service on it.
#[derive(Debug)]
pub(crate) struct ScreenShot2 {
    connection: Connection,
}

impl ScreenShot2 {
    /// Connects to the session bus; `None` when the service is not on it.
    /// No call on the connection waits longer than `patience`.
    pub(crate) fn connect(patience: Duration) -> Result<Option<Self>, ConnectError> {
        zbus::block_on(async {
            let connection = Bus::Session
                .builder()
                .method_timeout(patience)
                .build()
                .await
                .map_err(ConnectError::Bus)?;
            let reply = connection
                .call_method(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    Some("org.freedesktop.DBus"),
                    "NameHasOwner",
                    &INTERFACE,
                )
                .await
                .map_err(ConnectError::Bus)?;
            let owned: bool = reply.body().deserialize().map_err(ConnectError::Bus)?;
            Ok(owned.then_some(Self { connection }))
        })
    }

    /// Captures the output named `name`, at its physical resolution and
    /// without the cursor, by `deadline` (the call itself is bounded by the
    /// connection's timeout, at most the same).
    pub(crate) fn capture(&self, name: &str, deadline: Deadline) -> Result<Image, CaptureError> {
        let mut sink = Sink::new().map_err(|source| CaptureError::Io {
            what: "make the screenshot file",
            source,
        })?;
        let Some(writer) = sink.writer() else {
            return Err(CaptureError::Io {
                what: "make the screenshot file",
                source: io::Error::other("its write side is gone"),
            });
        };
        // KWin 6.6 leaves the caller's own windows out unless told not to:
        // a test that captures the windows it opened would see none.
        let options: HashMap<&str, Variant<'_>> = HashMap::from([
            ("native-resolution", Variant::from(true)),
            ("include-cursor", Variant::from(false)),
            ("hide-caller-windows", Variant::from(false)),
        ]);
        let reply = zbus::block_on(self.connection.call_method(
            Some(INTERFACE),
            PATH,
            Some(INTERFACE),
            "CaptureScreen",
            &(name, options, Fd::from(writer)),
        ));
        // KWin holds its own copy, and writes after it has answered; the
        // writing has ended when KWin closes its copy.
        sink.close_writer();
        let timed_out = || CaptureError::TimedOut {
            output: name.to_owned(),
            patience: deadline.patience(),
        };
        let reply = reply.map_err(|error| match &error {
            zbus::Error::InputOutput(io) if io.kind() == io::ErrorKind::TimedOut => timed_out(),
            _ => refusal(error, name),
        })?;
        let bytes = sink.wait_written(deadline).map_err(|source| {
            if source.kind() == io::ErrorKind::TimedOut {
                timed_out()
            } else {
                CaptureError::Io {
                    what: "read the screenshot file",
                    source,
                }
            }
        })?;
        let description: HashMap<String, OwnedVariant> =
            reply.body().deserialize().map_err(CaptureError::Bus)?;
        let description = Description::read(&description)?;
        Buffer {
            bytes: &bytes,
            width: description.width,
            height: description.height,
            stride: description.stride,
            layout: description.layout,
            y_inverted: false,
            // KWin renders the output's area upright.
            transform: Transform::Normal,
        }
        .to_image()
    }
}

/// What KWin's error means for the caller.
fn refusal(error: zbus::Error, output: &str) -> CaptureError {
    if let zbus::Error::MethodError(name, message, _) = &error {
        let detail = message.clone().unwrap_or_default();
        match name.as_str() {
            "org.kde.KWin.ScreenShot2.Error.NoAuthorized" => {
                return CaptureError::NotAuthorized(detail);
            }
            "org.kde.KWin.ScreenShot2.Error.InvalidScreen" => {
                return CaptureError::UnknownOutput {
                    name: output.to_owned(),
                    known: Vec::new(),
                };
            }
            _ => {}
        }
    }
    CaptureError::Bus(error)
}

/// The image KWin describes in its answer.
#[derive(Debug, PartialEq, Eq)]
struct Description {
    width: u32,
    height: u32,
    stride: u32,
    layout: Layout,
}

impl Description {
    fn read(map: &HashMap<String, OwnedVariant>) -> Result<Self, CaptureError> {
        let number = |key: &str| -> Result<u32, CaptureError> {
            map.get(key)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| CaptureError::BadBuffer(format!("KWin's answer has no {key}")))
        };
        let kind = map
            .get("type")
            .and_then(|value| <&str>::try_from(value).ok());
        if kind != Some("raw") {
            return Err(CaptureError::UnsupportedFormat(format!(
                "KWin image type {kind:?}"
            )));
        }
        let format = number("format")?;
        let layout = qimage_layout(format)
            .ok_or_else(|| CaptureError::UnsupportedFormat(format!("QImage format {format}")))?;
        Ok(Self {
            width: number("width")?,
            height: number("height")?,
            stride: number("stride")?,
            layout,
        })
    }
}

/// The byte layout of a `QImage::Format` on a little-endian machine, for
/// the 32-bit formats this crate reads.
fn qimage_layout(format: u32) -> Option<Layout> {
    if cfg!(target_endian = "big") {
        return None;
    }
    match format {
        // Format_RGB32, Format_ARGB32, Format_ARGB32_Premultiplied: one
        // 32-bit 0xAARRGGBB word per pixel.
        4 => Some(Layout::Bgrx),
        5 => Some(Layout::BgraStraight),
        6 => Some(Layout::Bgra),
        // Format_RGBX8888, Format_RGBA8888, Format_RGBA8888_Premultiplied:
        // bytes in R, G, B, A order.
        16 => Some(Layout::Rgbx),
        17 => Some(Layout::RgbaStraight),
        18 => Some(Layout::Rgba),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(entries: &[(&str, Variant<'_>)]) -> HashMap<String, OwnedVariant> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.try_to_owned().unwrap()))
            .collect()
    }

    #[test]
    fn reads_kwins_answer() {
        // What KWin 6.6 answers for a 1280×720 output.
        let map = answer(&[
            ("type", Variant::from("raw")),
            ("format", Variant::from(6_u32)),
            ("width", Variant::from(1280_u32)),
            ("height", Variant::from(720_u32)),
            ("stride", Variant::from(5120_u32)),
            ("scale", Variant::from(1.0_f64)),
            ("screen", Variant::from("Virtual-0")),
        ]);
        assert_eq!(
            Description::read(&map).unwrap(),
            Description {
                width: 1280,
                height: 720,
                stride: 5120,
                layout: Layout::Bgra,
            }
        );
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        let png = answer(&[("type", Variant::from("png"))]);
        assert!(matches!(
            Description::read(&png),
            Err(CaptureError::UnsupportedFormat(_))
        ));
        let rgb16 = answer(&[
            ("type", Variant::from("raw")),
            ("format", Variant::from(7_u32)),
        ]);
        assert!(matches!(
            Description::read(&rgb16),
            Err(CaptureError::UnsupportedFormat(_))
        ));
        let no_size = answer(&[
            ("type", Variant::from("raw")),
            ("format", Variant::from(4_u32)),
        ]);
        assert!(matches!(
            Description::read(&no_size),
            Err(CaptureError::BadBuffer(_))
        ));
    }
}

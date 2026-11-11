//! KWin's `org.kde.KWin.ScreenShot2` on the session bus: for KWin, which
//! offers neither Wayland capture protocol.
//!
//! `CaptureScreen` takes the output's name, options and the write end of a
//! pipe, answers with the image's description (size, stride, a `QImage`
//! format), and writes the raw pixels into the pipe. KWin answers only
//! callers it has granted the interface to; see the `deskrinch-kwin-access`
//! crate.

use std::collections::HashMap;
use std::io::{self, Read as _};
use std::os::fd::AsFd;
use std::thread;
use std::time::Duration;

use deskrinch_test_wayland::Deadline;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use zbus::{Connection, Fd, OwnedVariant, Variant};

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
            let connection = zbus::connection::Builder::session()
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
        let (reader, writer) = io::pipe().map_err(|source| CaptureError::Io {
            what: "make the screenshot pipe",
            source,
        })?;
        // KWin writes once it has answered; read alongside, so neither side
        // waits on a full pipe.
        let pixels = thread::spawn(move || read_until(&reader, deadline));
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
            &(name, options, Fd::from(writer.as_fd())),
        ));
        // KWin holds its own copy; the read ends when KWin closes it.
        drop(writer);
        let read = pixels.join().map_err(|_| CaptureError::Io {
            what: "read the screenshot pipe",
            source: io::Error::other("the reading thread panicked"),
        })?;
        let timed_out = || CaptureError::TimedOut {
            output: name.to_owned(),
            patience: deadline.patience(),
        };
        let reply = reply.map_err(|error| match &error {
            zbus::Error::InputOutput(io) if io.kind() == io::ErrorKind::TimedOut => timed_out(),
            _ => refusal(error, name),
        })?;
        let bytes = read.map_err(|source| {
            if source.kind() == io::ErrorKind::TimedOut {
                timed_out()
            } else {
                CaptureError::Io {
                    what: "read the screenshot pipe",
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

/// Reads `reader` to its end, or until `deadline` passes (an error of kind
/// `TimedOut`).
fn read_until(reader: &io::PipeReader, deadline: Deadline) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = vec![0; 1 << 16];
    loop {
        let left = deadline.left();
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let timeout = Timespec::try_from(left).unwrap_or(Timespec {
            tv_sec: i64::MAX,
            tv_nsec: 0,
        });
        let mut fds = [PollFd::new(reader, PollFlags::IN)];
        match poll(&mut fds, Some(&timeout)) {
            Ok(0) | Err(rustix::io::Errno::INTR) => continue,
            Ok(_) => {}
            Err(errno) => return Err(errno.into()),
        }
        match (&*reader).read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
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
    use std::io::Write as _;

    use super::*;

    #[test]
    fn a_pipe_nobody_closes_times_out() {
        let (reader, _writer) = io::pipe().unwrap();
        let error = read_until(&reader, Deadline::after(Duration::from_millis(200))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn a_pipe_is_read_to_its_end() {
        let (reader, mut writer) = io::pipe().unwrap();
        let sent = vec![7; 200_000];
        let written = thread::spawn({
            let sent = sent.clone();
            move || writer.write_all(&sent)
        });
        let bytes = read_until(&reader, Deadline::after(Duration::from_secs(600))).unwrap();
        written.join().unwrap().unwrap();
        assert_eq!(bytes, sent);
    }

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

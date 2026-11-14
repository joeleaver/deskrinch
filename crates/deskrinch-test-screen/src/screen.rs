//! [`Screen`]: a connection that captures outputs.

use std::fmt;
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_test_wait::{Deadline, PATIENCE};

use crate::error::{CaptureError, ConnectError};
use crate::image::Image;
use crate::kwin::ScreenShot2;
use crate::wayland::{Protocol, Wayland};

/// How often [`Screen::capture_until`] looks again.
const RETRY: Duration = Duration::from_millis(50);

/// How a [`Screen`] captures: the first of these the compositor offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Method {
    /// `ext-image-copy-capture-v1` with `ext-image-capture-source-v1`'s
    /// output sources: the standard protocol (sway, labwc).
    ExtImageCopyCapture,
    /// `wlr-screencopy-unstable-v1`: wlroots compositors before they offered
    /// the standard protocol.
    WlrScreencopy,
    /// KWin's `org.kde.KWin.ScreenShot2` on the session bus, for a
    /// compositor that offers neither protocol (KWin). The calling
    /// executable must be granted it; see
    /// [`KWIN_SCREENSHOT_INTERFACE`](crate::KWIN_SCREENSHOT_INTERFACE).
    KwinScreenShot2,
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ExtImageCopyCapture => "ext-image-copy-capture-v1",
            Self::WlrScreencopy => "wlr-screencopy-unstable-v1",
            Self::KwinScreenShot2 => "org.kde.KWin.ScreenShot2",
        })
    }
}

/// A connection to the session's compositor (`WAYLAND_DISPLAY`) and, when
/// needed, its session bus (`DBUS_SESSION_BUS_ADDRESS`), that captures what
/// its outputs show.
///
/// The outputs are those there were when it connected, by their
/// `wl_output` names. Each capture is synchronous: it returns once the
/// compositor has copied the output.
#[derive(Debug)]
pub struct Screen {
    wayland: Wayland,
    kwin: Option<ScreenShot2>,
}

impl Screen {
    /// Connects, and chooses how to capture from what the compositor
    /// offers: [`Method::ExtImageCopyCapture`], else
    /// [`Method::WlrScreencopy`], else [`Method::KwinScreenShot2`] when
    /// KWin's service is on the session bus.
    ///
    /// # Errors
    ///
    /// When there is no compositor to connect to, or it offers no way to
    /// capture.
    pub fn connect() -> Result<Self, ConnectError> {
        let wayland = Wayland::connect(|_| true)?;
        let kwin = if wayland.protocol().is_some() {
            None
        } else {
            Some(ScreenShot2::connect(PATIENCE)?.ok_or(ConnectError::NoMethod)?)
        };
        Ok(Self { wayland, kwin })
    }

    /// Connects and captures with `method`, whatever else the compositor
    /// offers: to check one method where a compositor offers several.
    ///
    /// # Errors
    ///
    /// [`ConnectError::NotOffered`] when the compositor does not offer
    /// `method`; otherwise as [`connect`](Self::connect).
    pub fn connect_with(method: Method) -> Result<Self, ConnectError> {
        let wayland = Wayland::connect(|offered| offered == method)?;
        let kwin = match method {
            Method::KwinScreenShot2 => {
                Some(ScreenShot2::connect(PATIENCE)?.ok_or(ConnectError::NotOffered(method))?)
            }
            Method::ExtImageCopyCapture | Method::WlrScreencopy => {
                if wayland.protocol().is_none() {
                    return Err(ConnectError::NotOffered(method));
                }
                None
            }
        };
        Ok(Self { wayland, kwin })
    }

    /// How this connection captures.
    #[must_use]
    pub fn method(&self) -> Method {
        match (self.wayland.protocol(), &self.kwin) {
            (Some(Protocol::Ext(_)), _) => Method::ExtImageCopyCapture,
            (Some(Protocol::Wlr(_)), _) => Method::WlrScreencopy,
            (None, _) => Method::KwinScreenShot2,
        }
    }

    /// The outputs' names, in the order the compositor announced them.
    #[must_use]
    pub fn outputs(&self) -> Vec<String> {
        self.wayland.output_names()
    }

    /// What the output named `output` shows now: see [`Image`] for what
    /// its pixels are. The cursor is left out.
    ///
    /// # Errors
    ///
    /// When there is no such output, the compositor fails the capture or
    /// offers no pixel format this crate reads, KWin has not granted the
    /// executable its screenshot interface, or the connection fails; and
    /// [`CaptureError::TimedOut`] when the compositor has not finished the
    /// capture within [`PATIENCE`](deskrinch_test_wait::PATIENCE).
    pub fn capture(&mut self, output: &str) -> Result<Image, CaptureError> {
        match &self.kwin {
            Some(kwin) => {
                // Named as the compositor names it, for the same errors.
                self.wayland.output_index(output)?;
                kwin.capture(output, Deadline::after(PATIENCE))
            }
            None => self.wayland.capture(output, PATIENCE),
        }
    }

    /// Captures `output` until `accept` takes what it shows, and returns that
    /// image; for content a client has only just committed, which the
    /// compositor shows a frame or more later.
    ///
    /// It returns within `patience` plus one capture: it starts no capture
    /// once `patience` has passed, and each capture is bounded by
    /// [`PATIENCE`](deskrinch_test_wait::PATIENCE).
    ///
    /// # Errors
    ///
    /// [`CaptureError::NotSeen`], with the last image, when `accept` took
    /// none within `patience`; otherwise as [`capture`](Self::capture).
    pub fn capture_until(
        &mut self,
        output: &str,
        patience: Duration,
        accept: impl Fn(&Image) -> bool,
    ) -> Result<Image, CaptureError> {
        let deadline = Instant::now() + patience;
        loop {
            let image = self.capture(output)?;
            if accept(&image) {
                return Ok(image);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(CaptureError::NotSeen {
                    output: output.to_owned(),
                    patience,
                    last: Box::new(image),
                });
            }
            thread::sleep(RETRY.min(left));
        }
    }
}

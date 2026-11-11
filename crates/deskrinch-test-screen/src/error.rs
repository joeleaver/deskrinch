//! What can go wrong.

use std::io;
use std::time::Duration;

use crate::image::Image;

/// Why [`Screen::connect`](crate::Screen::connect) failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConnectError {
    /// There is no compositor to connect to, or the connection failed.
    #[error("could not connect to the compositor")]
    Wayland(#[source] wayland_client::ConnectError),
    /// The compositor's globals could not be listed.
    #[error("could not list the compositor's globals")]
    Globals(#[source] wayland_client::globals::GlobalError),
    /// A global the capture needs could not be bound.
    #[error("could not bind {interface}")]
    Bind {
        /// The global.
        interface: &'static str,
        /// Why.
        #[source]
        source: wayland_client::globals::BindError,
    },
    /// The connection failed while learning the outputs.
    #[error("the connection to the compositor failed")]
    Dispatch(#[source] wayland_client::DispatchError),
    /// The compositor offers neither capture protocol, and no
    /// `org.kde.KWin.ScreenShot2` is on the session bus.
    #[error(
        "the compositor offers no way to capture its outputs: neither \
         ext_image_copy_capture_manager_v1 with \
         ext_output_image_capture_source_manager_v1, nor \
         zwlr_screencopy_manager_v1, and org.kde.KWin.ScreenShot2 is not on \
         the session bus"
    )]
    NoMethod,
    /// [`Screen::connect_with`](crate::Screen::connect_with) asked for a
    /// method the compositor does not offer.
    #[error("the compositor does not offer {0}")]
    NotOffered(crate::Method),
    /// The session bus could not be asked about KWin's screenshot service.
    #[error("could not ask the session bus for org.kde.KWin.ScreenShot2")]
    Bus(#[source] zbus::Error),
}

/// Why a capture failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CaptureError {
    /// No output has this name.
    #[error("there is no output named {name:?} (there are {known:?})")]
    UnknownOutput {
        /// The name asked for.
        name: String,
        /// The outputs there are.
        known: Vec<String>,
    },
    /// The compositor offers the output's pixels only in formats this crate
    /// does not read.
    #[error("the compositor offers only pixel formats this crate does not read: {0}")]
    UnsupportedFormat(String),
    /// The compositor reported that it could not capture the output.
    #[error("the compositor failed the capture: {0}")]
    Failed(String),
    /// KWin refused the screenshot: no installed desktop file for this
    /// executable lists [`KWIN_SCREENSHOT_INTERFACE`](crate::KWIN_SCREENSHOT_INTERFACE)
    /// in `X-KDE-DBUS-Restricted-Interfaces` (see the `deskrinch-kwin-access`
    /// crate).
    #[error(
        "KWin did not grant this executable org.kde.KWin.ScreenShot2: {0} \
         (see the deskrinch-kwin-access crate)"
    )]
    NotAuthorized(String),
    /// The buffer the compositor filled does not hold what its description
    /// says.
    #[error("the captured buffer is malformed: {0}")]
    BadBuffer(String),
    /// The connection to the compositor failed.
    #[error("the connection to the compositor failed")]
    Dispatch(#[source] wayland_client::DispatchError),
    /// The buffer or the pipe the pixels arrive through failed.
    #[error("could not {what}")]
    Io {
        /// What was being done.
        what: &'static str,
        /// Why it failed.
        #[source]
        source: io::Error,
    },
    /// Talking to KWin over D-Bus failed.
    #[error("the D-Bus call to KWin failed")]
    Bus(#[source] zbus::Error),
    /// [`Screen::capture_until`](crate::Screen::capture_until) never saw what
    /// it waited for.
    #[error("{output} did not show what was waited for within {patience:?} ({last:?})")]
    NotSeen {
        /// The output.
        output: String,
        /// How long it waited.
        patience: Duration,
        /// The last image captured.
        last: Box<Image>,
    },
}

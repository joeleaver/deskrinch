//! Pictures of a nested session's outputs, for tests that check what a
//! shell component puts on screen.
//!
//! [`Screen::connect`] connects to the session's compositor and picks how
//! to capture from what it offers, never from its name ([`Method`]):
//!
//! 1. `ext-image-copy-capture-v1`, with `ext-image-capture-source-v1`'s
//!    output sources: the standard protocol. sway 1.11 and labwc 0.9 offer
//!    it.
//! 2. `wlr-screencopy-unstable-v1`, where the standard one is missing: older
//!    wlroots compositors.
//! 3. KWin's `org.kde.KWin.ScreenShot2` on the session bus, where neither
//!    is offered: KWin 6.6 offers neither. KWin answers only an executable
//!    whose installed desktop file grants it [`KWIN_SCREENSHOT_INTERFACE`]
//!    (the `deskrinch-kwin-access` crate writes one).
//!
//!    KWin offers `ScreenShot2` only with its plugins installed: Debian and
//!    Ubuntu ship them in `kwin-common`, which `kwin-wayland` only
//!    recommends. And it captures only while it composites with OpenGL; with
//!    `QPainter` it answers, but with nothing. A nested KWin's virtual backend
//!    composites with OpenGL only on a DRM device, so on a machine without a
//!    GPU it needs one such as vgem's (`modprobe vgem`, its nodes open to the
//!    user), and Mesa's software driver (`libgl1-mesa-dri`) to render with.
//!
//! [`Screen::capture`] returns an output's [`Image`]: upright, in the
//! output's physical pixels, as [`Rgba`] rows from the top. Outputs are
//! named as `wl_output` names them (`HEADLESS-1`, `Virtual-0`, ...);
//! [`Screen::outputs`] lists them. [`Screen::capture_until`] captures again
//! until the picture shows what a test waits for, since a client's commit
//! reaches the screen a frame or more later.
//!
//! The crate's compositor tests check, under all three compositors, outputs
//! at scale 1, at scale 2, turned a quarter from the start
//! (`cargo xtask session --transform 90`), and mirrored and turned
//! (`--transform flipped-90`): each image upright and unmirrored, at the
//! turned size, pixel for pixel. Under KWin, whose `ScreenShot2` renders the
//! output's area of its scene, that image comes upright from KWin itself;
//! under sway and labwc the crate undoes the transform the protocol reports.
//! An output turned under a running client is checked under sway and labwc,
//! which can change one at run time.
//!
//! No async runtime is needed: each capture blocks until the compositor
//! has copied the output, and never longer than [`CAPTURE_PATIENCE`]: a
//! compositor that does not answer fails it with
//! [`CaptureError::TimedOut`], naming the output. `Rgba` and `Transform`
//! are `deskrinch-test-wayland`'s, which `deskrinch-test-client` paints in.
//!
//! # Only in a nested session
//!
//! It captures whatever compositor `WAYLAND_DISPLAY` names, so a test that
//! uses it runs inside an isolated session (`cargo xtask session`, or
//! `cargo xtask compositor-tests` for a crate's compositor tests), never in
//! the real one.
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_test_screen::{Rgba, Screen};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut screen = Screen::connect()?;
//! let output = &screen.outputs()[0];
//! let image = screen.capture(output)?;
//! assert_eq!(image.pixel(0, 0), Some(Rgba::opaque(0, 0, 0)));
//! # Ok(())
//! # }
//! ```

mod buffer;
mod error;
mod image;
mod kwin;
mod screen;
mod wayland;

pub use deskrinch_test_wayland::{Rgba, Transform};
pub use error::{CaptureError, ConnectError};
pub use image::Image;
pub use screen::{CAPTURE_PATIENCE, Method, Screen};

/// The D-Bus interface KWin must grant an executable that captures under
/// KWin: the name its desktop file lists in
/// `X-KDE-DBUS-Restricted-Interfaces` (`deskrinch_kwin_access::DesktopEntry`).
pub const KWIN_SCREENSHOT_INTERFACE: &str = kwin::INTERFACE;

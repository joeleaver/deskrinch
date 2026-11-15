//! The exit statuses a component ends with besides success, as its starter
//! reads them: part of the contract between a component and whoever starts
//! it (the nested desktop, the login session's units).

/// The status a component ends with when its display went away: the
/// Wayland compositor it was a client of exited, crashed or dropped its
/// connection. sysexits' `EX_UNAVAILABLE` ("a service is unavailable").
///
/// It is neither 0 nor rinch's 1 on purpose. It is a failure under the
/// restart policy ([`Exit::of`](crate::restart::Exit::of)), so a starter
/// that restarts on failure starts the component again: under KWin,
/// `kwin_wayland_wrapper` restarts a crashed `kwin_wayland` on the same
/// socket, and the component, started again, connects to the new one. A
/// compositor gone for good makes the restarts fail to connect, and the
/// restart limit ends them; at a logout the session stops the components
/// itself.
pub const DISPLAY_LOST: u8 = 69;

/// What a component says, in one line, as it ends with [`DISPLAY_LOST`]:
/// the line its starter, or a reader of its log, finds.
pub const DISPLAY_LOST_LINE: &str = "the display went away; ending";

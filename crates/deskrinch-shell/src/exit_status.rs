//! How a component ends, as its starter reads it: part of the contract
//! between a component and whoever starts it (the nested desktop, the
//! login session's units).
//!
//! - **0, asked to stop** (`SIGTERM`, `SIGINT`, `SIGHUP`), or its app
//!   closed: it ends in order, having said [`asked_to_stop_line`] when it
//!   was asked. A second such signal while it ends, or a process still
//!   running a bound after the first, ends it by the signal instead, which
//!   the restart policy also reads as clean; a signal ignored when it
//!   started stays ignored. `deskrinch-component`'s `stop` module is how.
//!   From the moment it has said [`stops_in_order_line`] (at debug level),
//!   a stop signal ends it so; before, it is still starting, and the
//!   signal's default action ends it, by the signal.
//! - **[`DISPLAY_LOST`] (69), its display went away**, having said
//!   [`DISPLAY_LOST_LINE`]: restarted.
//! - **[`CANNOT_RUN_HERE`] (78), it cannot run where it was started**: not
//!   restarted.

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

/// The status a component ends with when it cannot run where it was
/// started, and starting it again there would not change that: the
/// session's bus name it serves belongs to another program, or the
/// compositor lacks a protocol the component cannot do without (the
/// wallpaper on a compositor with no layer shell). It says why in its log
/// first.
///
/// Under the restart policy it is neither clean nor a failure to restart
/// ([`Exit::CannotRunHere`](crate::restart::Exit::CannotRunHere)): the
/// component is not started again, where a plain failure would be started
/// up to the limit for nothing. A systemd unit states it as
/// `RestartPreventExitStatus=`
/// ([`Restart::systemd_lines`](crate::restart::Restart::systemd_lines)),
/// and leaves the unit failed, as it should be: the component is not
/// running.
///
/// It is sysexits' `EX_CONFIG`, "something was found in an unconfigured or
/// misconfigured state": the session the component was started in is not
/// set up for it. Not 1 (rinch's own failure, and any panic's handler's),
/// not [`DISPLAY_LOST`] (which is restarted), not 2 (argument errors), not
/// LSB's 1 to 7 (systemd gives each a meaning of its own), and not 200 and
/// above (systemd's own reasons a service did not start).
pub const CANNOT_RUN_HERE: u8 = 78;

/// What a component says, in one line at info level, when it is asked to
/// stop by `signal` (`SIGTERM`, `SIGINT` or `SIGHUP`, named so) and ends in
/// order, with success.
#[must_use]
pub fn asked_to_stop_line(signal: &str) -> String {
    format!("asked to stop by {signal}; ending")
}

/// What a component says, in one line at debug level, once a stop signal
/// ends it in order (see the module's list): process `pid` (its own), so
/// that a starter of several finds whose line it is.
#[must_use]
pub fn stops_in_order_line(pid: u32) -> String {
    format!("process {pid} now ends in order when asked to stop")
}

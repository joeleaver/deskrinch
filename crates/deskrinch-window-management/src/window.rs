//! What the crate reports: windows, their state, and changes to the list.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::EndReason;

/// Identifies one window for as long as the compositor reports it.
///
/// Ids are assigned by this crate, never reused within a process, and mean
/// nothing to the compositor or to another process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(u64);

impl WindowId {
    /// A fresh id, distinct from every id handed out before in this process.
    pub(crate) fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// The id as a number, for logs and for keying a caller's own maps.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "window {}", self.0)
    }
}

bitflags::bitflags! {
    /// The state of a window, as a set of flags.
    ///
    /// Not every compositor reports every flag:
    /// [`Capabilities::states`](crate::Capabilities::states) says which ones a
    /// connection reports. A flag the connection does not report is never
    /// set.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct WindowState: u8 {
        /// The window has keyboard focus.
        const ACTIVE = 1;
        /// The window is minimized (hidden until restored).
        const MINIMIZED = 1 << 1;
        /// The window is maximized.
        const MAXIMIZED = 1 << 2;
        /// The window is fullscreen.
        const FULLSCREEN = 1 << 3;
        /// The window asks for the user's attention ("urgent").
        const DEMANDS_ATTENTION = 1 << 4;
        /// The window asks not to be shown in a taskbar.
        const SKIP_TASKBAR = 1 << 5;
    }
}

/// One window as the compositor last reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Window {
    /// Which window this is.
    pub id: WindowId,
    /// Its title; empty until the compositor reports one.
    pub title: String,
    /// Its application id (on Wayland, the `xdg_toplevel` app id, usually the
    /// application's desktop file name); empty until the compositor reports
    /// one.
    pub app_id: String,
    /// Its state.
    pub state: WindowState,
    /// The outputs it is on, by `wl_output` name (the connector name, such as
    /// `DP-1`). Empty when the connection does not report outputs (see
    /// [`Capabilities::outputs`](crate::Capabilities::outputs)).
    pub outputs: BTreeSet<String>,
    /// The window this one belongs to, such as the main window of a dialog;
    /// `None` for a top-level window, and always `None` when the connection
    /// does not report parents (see
    /// [`Capabilities::parent`](crate::Capabilities::parent)). A taskbar
    /// usually lists only windows without one. When the parent goes, this
    /// becomes `None`.
    pub parent: Option<WindowId>,
}

impl Window {
    /// A window with nothing reported yet.
    pub(crate) fn new(id: WindowId) -> Self {
        Self {
            id,
            title: String::new(),
            app_id: String::new(),
            state: WindowState::empty(),
            outputs: BTreeSet::new(),
            parent: None,
        }
    }
}

/// A change to the window list, delivered to the subscriber.
///
/// For each window, `Added` comes first, then any number of `Changed`, then
/// at most one `Removed`. `Ended` is the last event of a connection.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    /// A window appeared, with everything the compositor reported about it.
    Added(Window),
    /// Something about a window changed; this is its new state in full.
    Changed(Window),
    /// The window is gone.
    Removed(WindowId),
    /// The compositor stopped reporting windows, or the connection was lost.
    /// No events follow; the window list is empty and actions fail.
    Ended(EndReason),
}

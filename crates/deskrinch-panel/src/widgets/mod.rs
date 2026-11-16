//! The panel's widgets, one module each. A widget is a component the
//! [`bar`](crate::bar) places; [`buttons`] is what their buttons share.

pub(crate) mod buttons;
mod clock;
mod switcher;
mod taskbar;

pub(crate) use clock::ClockWidget;
pub(crate) use switcher::{Following as FollowedWorkspaces, Runs, SwitcherWidget};
pub(crate) use taskbar::{AppIcons, Buttons, Following as FollowedWindows, TaskbarWidget};

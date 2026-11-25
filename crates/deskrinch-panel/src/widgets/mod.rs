//! The panel's widgets, one module each. A widget is a component the
//! [`bar`](crate::bar) places; [`buttons`] is what their buttons share.

pub(crate) mod buttons;
mod clock;
mod switcher;
mod taskbar;

#[cfg(test)]
pub(crate) use clock::ClockText;
pub(crate) use clock::{ClockWidget, follow as follow_clock};
pub(crate) use switcher::{Following as FollowedWorkspaces, Runs, SwitcherWidget};
pub(crate) use taskbar::{
    AppIcons, BarOutput, BarOutputs, Buttons, Following as FollowedWindows, TaskbarWidget,
    follow_graces,
};
#[cfg(test)]
pub(crate) use taskbar::{GraceClock, apply_window_change};

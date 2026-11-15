//! The panel's widgets, one module each. A widget is a component the
//! [`bar`](crate::bar) places.

mod clock;
mod taskbar;

pub(crate) use clock::ClockWidget;
pub(crate) use taskbar::{AppIcons, TaskbarWidget};

//! The panel's widgets, one module each. A widget is a component the
//! [`bar`](crate::bar) places in one of its regions.

mod clock;

pub(crate) use clock::ClockWidget;

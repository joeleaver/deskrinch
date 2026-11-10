//! What every deskrinch shell component process (the panel, the launcher,
//! ...) does the same way, on rinch.
//!
//! - [`report`]: logging through `tracing`, with the filter and format
//!   rinch's window uses, set up before anything is reported
//!   ([`report::init`]); errors with their causes, and configuration
//!   diagnostics at their severity, with their file, line and column.
//! - [`LiveSettings`]: the component's table of the configuration
//!   (a [`deskrinch_settings::Table`]), read when it starts and followed
//!   while it runs. It starts the process's one configuration watcher and
//!   subscribes the table; the settings it returns make the window, and every
//!   later change reaches the UI through a [`rinch` signal](rinch::prelude::Signal).
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_component::{LiveSettings, report};
//! use deskrinch_settings::panel::PanelSettings;
//! use deskrinch_xdg::BaseDirs;
//! use rinch::prelude::*;
//!
//! report::init();
//! let live = LiveSettings::<PanelSettings>::start(&BaseDirs::from_process());
//! let first = live.initial.clone();
//! // ... make the window from `first`, then in the root component:
//! # let _ = |_scope: &mut RenderScope| {
//! let settings = Signal::new(first.clone());
//! if let Some(newer) = live.connect(settings) {
//!     settings.set(newer);
//! }
//! # };
//! ```

mod live;
pub mod report;

pub use live::LiveSettings;

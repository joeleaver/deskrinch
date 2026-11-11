//! What every deskrinch shell component process (the panel, the launcher,
//! ...) does the same way, on rinch.
//!
//! - [`report`]: logging through `tracing`, with the filter and format
//!   rinch's window uses, set up before anything is reported
//!   ([`report::init`]); errors with their causes, and configuration
//!   diagnostics at their severity, with their file, line and column.
//! - [`Configuration`]: the process's configuration files and the one
//!   watcher on them, started once. Every table the component reads
//!   ([`deskrinch_settings::Table`]) is [followed](Configuration::follow)
//!   from it, as a [`LiveSettings`].
//! - [`LiveSettings`]: one table, read when the component starts
//!   ([`initial`](LiveSettings::initial), which the window is made from)
//!   and followed while it runs; in the root component,
//!   [`into_signal`](LiveSettings::into_signal) hands it to the UI as a
//!   [`rinch` signal](rinch::prelude::Signal) that every change reaches.
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_component::{Configuration, report};
//! use deskrinch_settings::panel::PanelSettings;
//! use deskrinch_xdg::BaseDirs;
//! use rinch::prelude::*;
//!
//! report::init();
//! let configuration = Configuration::start(&BaseDirs::from_process());
//! let panel = configuration.follow::<PanelSettings>();
//! let height = panel.initial.height.get();
//! // ... make the window `height` high, then in the root component:
//! # let _ = |_scope: &mut RenderScope| {
//! let settings: Signal<PanelSettings> = panel.into_signal();
//! # };
//! ```

mod configuration;
#[cfg(test)]
mod configuration_tests;
mod live;
pub mod report;

pub use configuration::Configuration;
pub use live::LiveSettings;

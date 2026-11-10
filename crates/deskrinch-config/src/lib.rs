//! Every shell component's settings: layered TOML files, typed sections,
//! precise diagnostics and live reload.
//!
//! # The file
//!
//! The configuration is one TOML file, [`deskrinch/config.toml`](FILE),
//! with a table per component (`[panel]`, `[launcher]`, ...) and `[general]`
//! for settings several components share. This crate knows no component's
//! settings: a component defines its section as a type and asks for it by
//! table name.
//!
//! # Layers
//!
//! From the lowest:
//!
//! 1. the component's built-in defaults, its type's [`Default`];
//! 2. the file in each of `XDG_CONFIG_DIRS`, the last directory lowest;
//! 3. the user's file in `XDG_CONFIG_HOME`.
//!
//! Tables merge key by key; arrays and other values replace. [`Sources`]
//! names the files; [`deskrinch_xdg::BaseDirs`] says where the directories
//! are.
//!
//! # Problems are diagnostics, never failures
//!
//! A file that is not valid TOML, or whose section has a value of the wrong
//! type, is reported as a [`Diagnostic`] with its file, line and column and
//! what was expected, and that file is skipped: the component runs on the
//! layers below it. When a file that was good breaks while the component
//! runs, its last good version keeps being used instead. A key the section's
//! type does not have is a warning, with its location: that is how a typo
//! gets noticed.
//!
//! # Where values came from
//!
//! [`Resolved::effective`] lists every effective value and the layer it came
//! from; its [`Display`](std::fmt::Display) is what a `--print-config`
//! option prints.
//!
//! # Live
//!
//! A [`Watcher`] delivers the section's new settings when any layer's file
//! changes, is created or removed (including by an editor that saves by
//! renaming a new file over the old one). A change that leaves the
//! section's settings as they were delivers nothing.
//!
//! # Structure
//!
//! The model is pure: [`Snapshot::from_texts`] parses texts, and a
//! [`Section`] layers, merges and validates them. Only [`Snapshot::read`],
//! [`load`] and [`Watcher`] touch the file system. No function needs an
//! async runtime: the watcher runs on a thread of its own and calls back.
//!
//! # Example
//!
//! ```
//! use deskrinch_config::{Event, Sources, Watcher, load};
//! use deskrinch_xdg::BaseDirs;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//! #[serde(rename_all = "kebab-case")]
//! enum Edge {
//!     Top,
//!     Bottom,
//! }
//!
//! #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//! struct Panel {
//!     edge: Edge,
//!     height: u32,
//! }
//!
//! impl Default for Panel {
//!     fn default() -> Self {
//!         Self { edge: Edge::Top, height: 32 }
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let dir = tempfile::tempdir()?;
//! # let vars = |name: &str| match name {
//! #     "XDG_CONFIG_HOME" => Some(dir.path().join("home").into_os_string()),
//! #     "XDG_CONFIG_DIRS" => Some(dir.path().join("etc").into_os_string()),
//! #     _ => None,
//! # };
//! # let dirs = BaseDirs::from_vars(vars);
//! // In a component: `BaseDirs::from_process()`.
//! let sources = Sources::new(&dirs);
//!
//! // Once:
//! let panel = load::<Panel>(&sources, "panel")?;
//! for diagnostic in &panel.diagnostics {
//!     eprintln!("{diagnostic}");
//! }
//! print!("{}", panel.effective);
//! assert_eq!(panel.settings, Panel::default());
//!
//! // Or live:
//! let (watcher, panel) = Watcher::start(sources, "panel", |event: Event<Panel>| match event {
//!     Event::Changed(panel) => println!("now {panel:?}"),
//!     Event::Diagnostics(diagnostics) => {
//!         diagnostics.iter().for_each(|d| eprintln!("{d}"));
//!     }
//!     Event::Error(error) => eprintln!("{error}"),
//! })?;
//! # drop((watcher, panel));
//! # Ok(())
//! # }
//! ```

mod diagnostic;
mod effective;
mod error;
mod section;
mod snapshot;
mod sources;
mod tree;
mod watch;

pub use diagnostic::{Diagnostic, Location, Problem, Severity};
pub use effective::{Effective, EffectiveValue, ValueSource};
pub use error::{BoxedError, DefaultsError, WatchError};
pub use section::{Resolved, Section, Settings};
pub use snapshot::Snapshot;
pub use sources::{FILE, Sources, load};
pub use watch::{Event, Watcher};

#[cfg(test)]
mod tests;

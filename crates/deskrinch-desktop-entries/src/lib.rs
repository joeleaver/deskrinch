//! The applications installed on the system: finding them, searching them,
//! launching them and watching for changes, by the freedesktop
//! [Desktop Entry Specification].
//!
//! # Model
//!
//! - An [`Environment`] says where to look (the XDG data directories), what
//!   desktop is running, which locales the user prefers and where the
//!   session bus is. It is a value the caller builds, usually with
//!   [`Environment::from_process`]; nothing below it reads the process
//!   environment, so everything runs on fixtures.
//! - A [`DesktopEntry`] is one parsed `.desktop` file, named by its
//!   [`DesktopId`]. Translated keys are [`Localized`] values, resolved by the
//!   specification's locale matching when read.
//! - A [`Catalog`] is every installed entry, one per ID, with the
//!   specification's precedence and `Hidden` rules applied and each entry's
//!   [`Visibility`] (`NoDisplay`, `OnlyShowIn`/`NotShowIn`, `TryExec`, and
//!   whether the program `Exec` runs is installed).
//! - [`entry_for_app_id`] finds the application a window belongs to, by the
//!   application id the window carries.
//! - [`search()`] ranks entries against a query; the ranking is specified
//!   in [`search`'s module](mod@crate::search).
//! - [`launch`] runs an entry: `Exec` expanded by [`exec::expand`], in a
//!   terminal when `Terminal=true`, or by D-Bus activation when
//!   `DBusActivatable=true`. Policy such as the terminal emulator comes in a
//!   [`LaunchContext`]; [`default_terminal`] is `xdg-terminal-exec` when it
//!   is installed, for a caller whose user named none.
//! - A [`Watcher`] reports entries added, changed and removed as the data
//!   directories change.
//!
//! Parsing, precedence, ranking and `Exec` expansion are pure; only
//! [`Catalog::scan`], [`launch`] and [`Watcher`] touch the system. No
//! function needs an async runtime: the watcher runs on a thread of its own
//! and calls back.
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_desktop_entries::{Catalog, Environment, LaunchContext, launch, search};
//!
//! let env = Environment::from_process();
//! let catalog = Catalog::scan(&env);
//! let found = search(catalog.shown(), "term", &env.locales);
//! if let Some(best) = found.first() {
//!     let context = LaunchContext::new(&env)
//!         .with_terminal(["foot", "--"])
//!         .with_scope_launcher("deskrinch");
//!     launch(best.entry, None, &[], &context)?;
//! }
//! # Ok::<(), deskrinch_desktop_entries::LaunchError>(())
//! ```
//!
//! [Desktop Entry Specification]: https://specifications.freedesktop.org/desktop-entry/latest/

mod app_id;
mod catalog;
mod deviation;
mod entry;
mod env;
mod error;
pub mod exec;
mod fold;
mod icon;
mod launch;
mod scan;
pub mod search;
mod watch;

pub use app_id::entry_for_app_id;
pub use catalog::{Catalog, CatalogEntry, Visibility};
pub use deskrinch_keyfile::{
    Deviation as KeyFileDeviation, Locale, Localized, LocalizedList, LocalizedString, SyntaxError,
};
pub use deskrinch_watch::WatchError;
pub use deskrinch_xdg::BaseDirs;
pub use deviation::Deviation;
pub use entry::{Action, Application, DesktopEntry, DesktopId, EntryKind};
pub use env::Environment;
pub use error::{BoxedError, ExecError, LaunchError, ParseError, Problem, ProblemKind, ScanError};
pub use exec::Target;
pub use icon::Icon;
pub use launch::{
    DirectReason, LaunchContext, Launched, Placement, XDG_TERMINAL_EXEC, default_terminal, launch,
};
pub use search::{Coverage, Match, MatchField, MatchKind, search};
pub use watch::{Event, Watcher};

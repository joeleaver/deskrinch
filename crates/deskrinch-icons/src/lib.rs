//! Icon names resolved to files by the freedesktop [Icon Theme
//! Specification]: for the launcher, the taskbar, the tray and
//! notifications.
//!
//! # Model
//!
//! - A [`SearchPath`] is the base directories themes live in, most
//!   important first: `~/.icons`, `icons` in each XDG data directory, and
//!   `/usr/share/pixmaps`. [`SearchPath::new`] takes them from a
//!   [`deskrinch_xdg::BaseDirs`].
//! - A [`Theme`] is one theme's `index.theme`: its parents (`Inherits`)
//!   and its [`Directory`]s, each `Fixed`, `Scalable` or `Threshold`, with
//!   a size and a scale.
//! - An [`Index`] is everything a lookup in one theme needs: that theme,
//!   every theme it inherits from, `hicolor`, and the unthemed icons
//!   directly in the base directories, with which icons each directory
//!   holds. [`Index::load`] builds it by listing the directories, by name:
//!   it does not follow links, which in a theme like Breeze would cost most
//!   of the build. Nothing fails, and what was wrong is in
//!   [`Index::problems`].
//! - [`Index::candidates`] gives the files that could answer a [`Request`]
//!   (a name, a size, a scale, and optionally the generic fallback from
//!   `a-b-c` to `a-b` to `a`), best first, by the specification's lookup;
//!   it reads no file. [`Index::lookup`] is the first of them, an [`Icon`]
//!   (a path and its [`Format`]) or a [`LookupError`].
//! - [`Index::find`] is the first that exists, so a broken link is never
//!   returned, at one `stat` per candidate checked. It also answers an
//!   absolute path, as itself, when the file exists.
//! - A [`Watcher`] follows the themes through `deskrinch-watch` and
//!   delivers a new index when they change.
//!
//! Which theme is the user's is a setting, not this crate's business: the
//! theme's name is an argument.
//!
//! # Example
//!
//! ```
//! use deskrinch_icons::{Index, Request, SearchPath};
//!
//! # let dir = tempfile::tempdir().unwrap();
//! # let hicolor = dir.path().join("icons/hicolor");
//! # std::fs::create_dir_all(hicolor.join("48x48/apps")).unwrap();
//! # std::fs::write(hicolor.join("index.theme"),
//! #     "[Icon Theme]\nName=Hicolor\nDirectories=48x48/apps\n[48x48/apps]\nSize=48\nType=Fixed\n").unwrap();
//! # std::fs::write(hicolor.join("48x48/apps/foot.png"), "").unwrap();
//! # let dirs = vec![dir.path().join("icons")];
//! // In a component: `SearchPath::new(&BaseDirs::from_process())`.
//! let search_path = SearchPath::from_dirs(dirs);
//! let index = Index::load(&search_path, "breeze");
//! let icon = index.lookup(&Request::new("foot", 48).at_scale(1))?;
//! assert!(icon.path.ends_with("hicolor/48x48/apps/foot.png"));
//! # Ok::<(), deskrinch_icons::LookupError>(())
//! ```
//!
//! [Icon Theme Specification]: https://specifications.freedesktop.org/icon-theme/latest/

mod candidates;
mod index;
mod listing;
mod load;
mod lookup;
mod problem;
mod table;
mod theme;
mod watch;

pub use candidates::Candidates;
pub use index::{Format, Index};
pub use load::SearchPath;
pub use lookup::{Fallback, HICOLOR, Icon, LookupError, Request};
pub use problem::{Problem, ProblemKind};
pub use theme::{Directory, DirectoryKind, Theme};
pub use watch::{Event, Watcher};

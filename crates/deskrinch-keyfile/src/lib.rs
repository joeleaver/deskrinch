//! The freedesktop key-file format, which desktop entries (`.desktop`) and
//! icon themes (`index.theme`) are written in: groups of
//! `Key[locale]=value` lines, the escapes of `string` and list values, and
//! the matching of translations to locales.
//!
//! This crate knows nothing about what the keys mean; a format's reader
//! does. It reads files the way GLib's `GKeyFile` does. A file that breaks
//! the specification in a way GLib reads anyway is kept, and each
//! deviation is reported to the reader as a [`Deviation`]. Only what GLib
//! refuses outright fails the file, as a [`SyntaxError`].
//!
//! # Example
//!
//! ```
//! use deskrinch_keyfile::{Deviation, KeyFile};
//!
//! let text = "[Icon Theme]\nName=Example\nDirectories=48x48/apps;\n\n\
//!             [48x48/apps]\nSize=48\nType=Fixed\n";
//! let mut deviations = Vec::<Deviation>::new();
//! let file = KeyFile::parse(text.as_bytes(), &mut deviations)?;
//! let theme = file.group("Icon Theme").unwrap();
//! assert_eq!(theme.list("Directories", &mut deviations), ["48x48/apps"]);
//! let dir = file.group("48x48/apps").unwrap();
//! assert_eq!(dir.integer("Size", &mut deviations), Some(48));
//! assert!(deviations.is_empty());
//! # Ok::<(), deskrinch_keyfile::SyntaxError>(())
//! ```

mod error;
mod escape;
mod file;
mod group;
mod locale;

pub use error::{Deviation, Report, SyntaxError};
pub use escape::unescape;
pub use file::{Group, KeyFile};
pub use locale::{Locale, Localized, LocalizedList, LocalizedString};

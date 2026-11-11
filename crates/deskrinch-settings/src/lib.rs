//! The tables of deskrinch's configuration file, and the reference for it.
//!
//! The file is `deskrinch/config.toml` in `$XDG_CONFIG_HOME`, laid over the
//! same file in each of `$XDG_CONFIG_DIRS` (`deskrinch-config` reads and
//! layers them). It has one table per component that reads settings, and
//! a table exists here only once a component reads it:
//!
//! | Table | Read by | Reference |
//! |---|---|---|
//! | `[general]` | every component | [`general`] |
//! | `[panel]` | `deskrinch-panel` | [`panel`] |
//!
//! Any other top-level table is reported as unknown (usually a misspelling),
//! because [`section`] gives every component the [`KNOWN_TABLES`].
//!
//! Each table is a type implementing [`Table`], which names the table it is
//! read from, so a type cannot be read from another table's name. The types
//! say what the file may say (keys, defaults, ranges, and the messages for
//! values out of range); what a component makes of its settings stays in
//! the component. The crate has no UI dependency, so anything that needs
//! every table's schema (a settings app, a command that checks the file)
//! can depend on it.
//!
//! # Example
//!
//! ```
//! use deskrinch_config::Snapshot;
//! use deskrinch_settings::{panel::PanelSettings, section};
//!
//! let text = "[panel]\nheight = 48\n".to_owned();
//! let snapshot = Snapshot::from_texts([("config.toml".into(), Some(text))]);
//! let resolved = section::<PanelSettings>()?.resolve(&snapshot);
//! assert_eq!(resolved.settings.height.get(), 48);
//! # Ok::<(), deskrinch_config::DefaultsError>(())
//! ```

pub mod general;
pub mod panel;

use deskrinch_config::{DefaultsError, Section, Settings};

/// A table of the configuration file: a settings type and the name of the
/// table it is read from.
///
/// It is implemented only by this crate's `tables!` declaration, the one
/// place a table is declared, which also lists it in [`KNOWN_TABLES`]: a
/// table cannot exist without being known, or be known under another name.
/// The trait is sealed, so no other crate can implement it.
pub trait Table: Settings + sealed::Sealed {
    /// The table's name, as the file writes it (`panel` for `[panel]`).
    const NAME: &'static str;
}

mod sealed {
    /// Implemented by the `tables!` declaration alone.
    pub trait Sealed {}
}

/// Declares every table of the configuration file, each once: its type
/// implements [`Table`] with its name, and the names make
/// [`KNOWN_TABLES`].
macro_rules! tables {
    ($($table:ty => $name:literal),+ $(,)?) => {
        $(
            impl sealed::Sealed for $table {}
            impl Table for $table {
                const NAME: &'static str = $name;
            }
        )+

        /// The name of every table of the configuration file.
        pub const KNOWN_TABLES: &[&str] = &[$($name),+];
    };
}

tables! {
    general::GeneralSettings => "general",
    panel::PanelSettings => "panel",
}

/// The section for table `T`, which reports any top-level table that is
/// not one of the [`KNOWN_TABLES`].
///
/// # Errors
///
/// [`DefaultsError`] when `T`'s defaults do not make a TOML table that
/// reads back as `T`: a bug in `T`, which this crate's tests catch.
pub fn section<T: Table>() -> Result<Section<T>, DefaultsError> {
    Ok(Section::new(T::NAME)?.with_known_tables(KNOWN_TABLES.iter().copied()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskrinch_config::{Problem, Snapshot};
    use std::collections::BTreeSet;

    #[test]
    fn every_table_has_a_name_of_its_own() {
        let names: BTreeSet<_> = KNOWN_TABLES.iter().collect();
        assert_eq!(names.len(), KNOWN_TABLES.len(), "{KNOWN_TABLES:?}");
        assert!(
            KNOWN_TABLES
                .iter()
                .all(|name| !name.is_empty() && !name.contains(['.', '[', ']', ' '])),
            "{KNOWN_TABLES:?}"
        );
    }

    #[test]
    fn every_tables_defaults_make_a_section() {
        fn check<T: Table>() {
            let section = section::<T>().unwrap();
            assert_eq!(section.name(), T::NAME);
            assert!(KNOWN_TABLES.contains(&section.name()));
        }
        check::<general::GeneralSettings>();
        check::<panel::PanelSettings>();
    }

    #[test]
    fn a_table_no_component_reads_is_unknown() {
        let text = "[launcher]\n[panel]\n".to_owned();
        let snapshot = Snapshot::from_texts([("config.toml".into(), Some(text))]);
        let resolved = section::<panel::PanelSettings>()
            .unwrap()
            .resolve(&snapshot);
        let problems: Vec<_> = resolved.diagnostics.iter().map(|d| &d.problem).collect();
        assert!(
            matches!(problems.as_slice(), [Problem::UnknownTable { table }] if table == "launcher"),
            "{problems:?}"
        );
    }
}

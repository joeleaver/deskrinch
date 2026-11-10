//! The tables of deskrinch's configuration file, and the reference for it.
//!
//! The file is `deskrinch/config.toml` in `$XDG_CONFIG_HOME`, laid over the
//! same file in each of `$XDG_CONFIG_DIRS` (`deskrinch-config` reads and
//! layers them). It has one table per component that reads settings, and
//! a table exists here only once a component reads it:
//!
//! | Table | Read by | Reference |
//! |---|---|---|
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

pub mod panel;

use deskrinch_config::{DefaultsError, Section, Settings};

/// A table of the configuration file: a settings type and the name of the
/// table it is read from.
pub trait Table: Settings {
    /// The table's name, as the file writes it (`panel` for `[panel]`).
    const NAME: &'static str;
}

/// The name of every table of the configuration file, from the tables'
/// own [`Table::NAME`]s.
pub const KNOWN_TABLES: &[&str] = &[panel::PanelSettings::NAME];

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

    /// Every table type, by its name; a new table is added here and to
    /// [`KNOWN_TABLES`].
    const TABLE_NAMES: &[&str] = &[panel::PanelSettings::NAME];

    #[test]
    fn every_table_is_known_once() {
        for name in TABLE_NAMES {
            assert_eq!(
                KNOWN_TABLES.iter().filter(|known| *known == name).count(),
                1,
                "{name}"
            );
        }
        assert_eq!(KNOWN_TABLES.len(), TABLE_NAMES.len());
    }

    #[test]
    fn every_tables_defaults_make_a_section() {
        assert_eq!(section::<panel::PanelSettings>().unwrap().name(), "panel");
    }

    #[test]
    fn a_table_no_component_reads_is_unknown() {
        let text = "[general]\n[panel]\n".to_owned();
        let snapshot = Snapshot::from_texts([("config.toml".into(), Some(text))]);
        let resolved = section::<panel::PanelSettings>()
            .unwrap()
            .resolve(&snapshot);
        let problems: Vec<_> = resolved.diagnostics.iter().map(|d| &d.problem).collect();
        assert!(
            matches!(problems.as_slice(), [Problem::UnknownTable { table }] if table == "general"),
            "{problems:?}"
        );
    }
}

//! Ways a desktop entry file can break the specification and still be read.
//!
//! A launcher that hides an application every other desktop shows has a
//! defect the user sees. So where a file deviates from the specification,
//! this crate reads it as GLib (`GKeyFile` and `GDesktopAppInfo`) does,
//! keeps the entry usable, and records the deviation. Each variant says
//! what was made of the file. The behaviour was established by experiment
//! against GLib 2.88; the tests in `leniency_tests.rs` cite each case.

/// A deviation from the specification in a file that was read anyway.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Deviation {
    /// A deviation in the key-file syntax every desktop entry is written
    /// in. A [`TranslationWithoutDefault`] of `Name` is read with
    /// `Unnamed` as its default; of another key, its translations are
    /// ignored.
    ///
    /// [`TranslationWithoutDefault`]: deskrinch_keyfile::Deviation::TranslationWithoutDefault
    #[error(transparent)]
    KeyFile(#[from] deskrinch_keyfile::Deviation),
    /// A key the specification requires is missing. A missing `Name` reads
    /// as `Unnamed`; a missing `Exec` leaves an entry that cannot be
    /// launched by command.
    #[error("the required key {key} is missing from [{group}]")]
    MissingKey {
        /// The group.
        group: String,
        /// The key.
        key: &'static str,
    },
    /// `Icon` is a name with a `.png`, `.xpm` or `.svg` extension, which a
    /// name should not have. It is read without it, as GLib reads it.
    #[error("Icon {value:?} in [{group}] has an image extension; read without it")]
    IconExtension {
        /// The group.
        group: String,
        /// The value as written.
        value: String,
    },
    /// `Icon` is a relative path. As in GLib, it is read as a name (without
    /// an image extension), which no icon theme has.
    #[error("Icon {value:?} in [{group}] is a relative path, read as a name")]
    RelativeIconPath {
        /// The group.
        group: String,
        /// The value as written.
        value: String,
    },
    /// `Actions` lists an action that has no `[Desktop Action …]` group. The
    /// action is ignored, as the specification says. (GLib lists it as
    /// `Unnamed`, an action with nothing to run; that is not copied.)
    #[error("action {action} is listed but has no group")]
    ActionWithoutGroup {
        /// The action's identifier.
        action: String,
    },
}

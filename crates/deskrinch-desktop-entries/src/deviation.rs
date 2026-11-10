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
    /// A group appears more than once. Its keys are merged into the first
    /// occurrence, later values replacing earlier ones.
    #[error("group [{group}] appears more than once; its keys were merged")]
    DuplicateGroup {
        /// The group.
        group: String,
    },
    /// A key appears more than once in a group. The last value is used.
    #[error("key {key} appears more than once in [{group}]; the last value is used")]
    DuplicateKey {
        /// The group.
        group: String,
        /// The key.
        key: String,
    },
    /// A key name uses characters other than `A-Za-z0-9-`. It is kept.
    #[error("key {key:?} in [{group}] has characters the specification does not allow")]
    NonstandardKey {
        /// The group.
        group: String,
        /// The key.
        key: String,
    },
    /// A line is not UTF-8. A key on it is ignored, its value unreadable; a
    /// group header still opens its group.
    #[error("line {line} is not UTF-8")]
    NotUtf8 {
        /// The 1-based line number.
        line: usize,
    },
    /// A value uses a backslash escape the specification does not define,
    /// or ends in a backslash. The backslash is kept literally.
    #[error("key {key} in [{group}] has an undefined escape, kept literally")]
    InvalidEscape {
        /// The group.
        group: String,
        /// The key.
        key: String,
    },
    /// A translation's locale cannot be matched (an encoding such as
    /// `de_DE.UTF-8`, or an empty `[]`). The translation is ignored.
    #[error("translation {key} in [{group}] has a locale that cannot be matched")]
    UnusableLocale {
        /// The group.
        group: String,
        /// The key with its locale, such as `Name[de_DE.UTF-8]`.
        key: String,
    },
    /// A key has translations but no unlocalised value. The translations
    /// are used for `Name`, with `Unnamed` as the fallback, and ignored for
    /// other keys.
    #[error("key {key} in [{group}] has translations but no unlocalised value")]
    TranslationWithoutDefault {
        /// The group.
        group: String,
        /// The key.
        key: String,
    },
    /// A boolean is neither `true` nor `false`. `1` reads as true, and
    /// anything else as false.
    #[error("key {key} in [{group}] is {value:?}, read as {read_as}")]
    InvalidBoolean {
        /// The group.
        group: String,
        /// The key.
        key: String,
        /// Its value.
        value: String,
        /// What it was read as.
        read_as: bool,
    },
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
    /// `Actions` lists an action that has no `[Desktop Action …]` group. The
    /// action is ignored, as the specification says. (GLib lists it as
    /// `Unnamed`, an action with nothing to run; that is not copied.)
    #[error("action {action} is listed but has no group")]
    ActionWithoutGroup {
        /// The action's identifier.
        action: String,
    },
}

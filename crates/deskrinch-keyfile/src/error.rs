//! What makes a key file unreadable, and what it may get away with.

/// Why a key file cannot be read at all: what GLib's `GKeyFile` refuses
/// too. What GLib reads despite the specifications is read here as well,
/// and reported as a [`Deviation`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyntaxError {
    /// A line is neither a comment, a group header nor a `Key=value`, or
    /// its group or key name is malformed.
    #[error("line {line} is malformed")]
    InvalidLine {
        /// The 1-based line number.
        line: usize,
    },
    /// A key comes before the first group header.
    #[error("line {line} has a key outside any group")]
    KeyOutsideGroup {
        /// The 1-based line number.
        line: usize,
    },
}

/// A way a key file breaks the specification and is read anyway, as GLib
/// reads it. Each variant says what was made of the file.
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
    /// A key has translations but no unlocalised value. What becomes of
    /// the translations is the reader's decision.
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
    /// An integer is not a whole number that fits. It reads as absent.
    #[error("key {key} in [{group}] is {value:?}, not an integer")]
    InvalidInteger {
        /// The group.
        group: String,
        /// The key.
        key: String,
        /// Its value.
        value: String,
    },
}

/// Where a reader's deviations go: a `Vec` of [`Deviation`], or of a
/// reader's own type that a [`Deviation`] converts into.
pub trait Report {
    /// Records `deviation`.
    fn report(&mut self, deviation: Deviation);
}

impl<T: From<Deviation>> Report for Vec<T> {
    fn report(&mut self, deviation: Deviation) {
        self.push(deviation.into());
    }
}

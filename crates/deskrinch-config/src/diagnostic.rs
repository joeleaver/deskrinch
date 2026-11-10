//! What is wrong with a configuration file, and where.

use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::tree::{Origin, SourceFile};

/// A problem found in a configuration file.
///
/// An [`Error`](Severity::Error) means the file, or its part of the
/// section, was not used; a [`Warning`](Severity::Warning) means it was used
/// and something in it is suspicious (an unknown key, usually a typo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The file.
    pub file: PathBuf,
    /// Where in the file, when the problem is at a particular place.
    pub location: Option<Location>,
    /// What is wrong.
    pub problem: Problem,
}

/// A place in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Location {
    /// The 1-based line.
    pub line: usize,
    /// The 1-based column, counted in characters.
    pub column: usize,
}

/// How serious a [`Diagnostic`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The file, or its part of the section, was skipped.
    Error,
    /// The file was used; something in it deserves a look.
    Warning,
}

/// What is wrong with a configuration file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Problem {
    /// The file exists but cannot be read. The whole file is skipped.
    #[error("cannot read the file: {message}")]
    Unreadable {
        /// The kind of I/O error.
        kind: io::ErrorKind,
        /// The I/O error's description.
        message: String,
    },
    /// The file is not UTF-8; the location is the first invalid byte. The
    /// whole file is skipped.
    #[error("the file is not UTF-8")]
    NotUtf8,
    /// The file is not valid TOML. The whole file is skipped.
    #[error("invalid TOML: {message}")]
    Syntax {
        /// The parser's description.
        message: String,
    },
    /// The section's key holds something other than a table
    /// (`panel = 3`). The file is skipped for this section.
    #[error("`{section}` must be a table, found {found}")]
    NotATable {
        /// The section.
        section: String,
        /// The TOML type found instead.
        found: &'static str,
    },
    /// A value the section's settings do not accept: the wrong type, or a
    /// value out of range or not among the choices. The file is skipped
    /// for this section.
    #[error("{}: {message}", Dotted(section, key))]
    Invalid {
        /// The section.
        section: String,
        /// The key within the section, when the problem is at one
        /// (`clock.format`, `widgets[2]`).
        key: Option<String>,
        /// What was found and what was expected.
        message: String,
    },
    /// A key the section's settings do not have. A warning: the rest of
    /// the file is used.
    #[error("unknown key `{}`", Dotted(section, &Some(key.clone())))]
    UnknownKey {
        /// The section.
        section: String,
        /// The key within the section.
        key: String,
    },
}

impl Problem {
    /// How serious the problem is.
    #[must_use]
    pub fn severity(&self) -> Severity {
        match self {
            Self::UnknownKey { .. } => Severity::Warning,
            _ => Severity::Error,
        }
    }
}

impl Diagnostic {
    /// How serious the problem is.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.problem.severity()
    }

    /// A diagnostic at a byte offset of a file's text.
    pub(crate) fn at(file: &SourceFile, offset: usize, problem: Problem) -> Self {
        Self {
            file: file.path.clone(),
            location: Some(Location::of(&file.text, offset)),
            problem,
        }
    }

    /// A diagnostic at a value's origin, which is in `file`. A value from
    /// the defaults, or from an earlier version of the file, has no
    /// location in it.
    pub(crate) fn at_origin(file: &SourceFile, origin: &Origin, problem: Problem) -> Self {
        match origin {
            Origin::File { file: from, span } if **from == *file => {
                Self::at(file, span.start, problem)
            }
            _ => Self {
                file: file.path.clone(),
                location: None,
                problem,
            },
        }
    }
}

impl Location {
    /// The line and column of byte `offset` in `text`.
    pub(crate) fn of(text: &str, offset: usize) -> Self {
        let before = text.get(..offset).unwrap_or(text);
        let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
        Self {
            line: before.matches('\n').count() + 1,
            column: before[line_start..].chars().count() + 1,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Error => "error",
            Self::Warning => "warning",
        })
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// `path:line:column: severity: problem`, the form editors and terminals
/// recognize.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.file.display())?;
        if let Some(location) = self.location {
            write!(f, ":{location}")?;
        }
        write!(f, ": {}: {}", self.severity(), self.problem)
    }
}

/// A section and an optional key within it, joined as TOML addresses them.
struct Dotted<'a>(&'a str, &'a Option<String>);

impl fmt::Display for Dotted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.1 {
            Some(key) if key.starts_with('[') => write!(f, "{}{key}", self.0),
            Some(key) => write!(f, "{}.{key}", self.0),
            None => f.write_str(self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_lines_and_character_columns() {
        let text = "a = 1\nbé = 'x'\n";
        assert_eq!(Location::of(text, 0), Location { line: 1, column: 1 });
        let offset = text.find("'x'").unwrap();
        assert_eq!(Location::of(text, offset), Location { line: 2, column: 6 });
        assert_eq!(Location::of(text, text.len()).line, 3);
    }

    #[test]
    fn displays_like_a_compiler() {
        let diagnostic = Diagnostic {
            file: "/c/config.toml".into(),
            location: Some(Location {
                line: 3,
                column: 10,
            }),
            problem: Problem::Invalid {
                section: "panel".into(),
                key: Some("height".into()),
                message: "invalid type: string \"tall\", expected u32".into(),
            },
        };
        assert_eq!(
            diagnostic.to_string(),
            "/c/config.toml:3:10: error: panel.height: invalid type: string \"tall\", expected u32"
        );
        let warning = Diagnostic {
            file: "/c/config.toml".into(),
            location: None,
            problem: Problem::UnknownKey {
                section: "panel".into(),
                key: "heigth".into(),
            },
        };
        assert_eq!(warning.severity(), Severity::Warning);
        assert_eq!(
            warning.to_string(),
            "/c/config.toml: warning: unknown key `panel.heigth`"
        );
    }
}

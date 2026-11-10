//! The configuration files as read at one moment, parsed.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use toml::de::DeTable;

use crate::diagnostic::{Diagnostic, Problem};
use crate::tree::{Node, SourceFile};

/// Every layer's file as read at one moment, lowest layer first, each
/// parsed or found broken.
///
/// [`Snapshot::read`] reads the files [`Sources`](crate::Sources) names;
/// [`Snapshot::from_texts`] builds one from texts, without I/O. A
/// [`Section`](crate::Section) resolves a component's settings from it.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub(crate) files: Vec<File>,
}

/// One layer's file.
#[derive(Debug, Clone)]
pub(crate) struct File {
    pub(crate) path: PathBuf,
    pub(crate) contents: Contents,
}

#[derive(Debug, Clone)]
pub(crate) enum Contents {
    /// There is no file: the layer is empty.
    Absent,
    /// The file is broken as a whole, and skipped.
    Broken(Diagnostic),
    /// The file parsed.
    Parsed { file: Arc<SourceFile>, root: Node },
}

impl Snapshot {
    /// A snapshot of files with these contents, lowest layer first: `None`
    /// for a file that does not exist, else its text.
    pub fn from_texts(files: impl IntoIterator<Item = (PathBuf, Option<String>)>) -> Self {
        let files = files
            .into_iter()
            .map(|(path, text)| match text {
                None => File {
                    path,
                    contents: Contents::Absent,
                },
                Some(text) => parse(path, text),
            })
            .collect();
        Self { files }
    }

    /// A snapshot of files with these contents, lowest layer first, as
    /// reading them returned: `Ok(None)` for a file that does not exist.
    pub(crate) fn from_reads(
        files: impl IntoIterator<Item = (PathBuf, io::Result<Option<Vec<u8>>>)>,
    ) -> Self {
        let files = files
            .into_iter()
            .map(|(path, read)| match read {
                Ok(None) => File {
                    path,
                    contents: Contents::Absent,
                },
                Ok(Some(bytes)) => decode(path, bytes),
                Err(error) => broken(
                    path.clone(),
                    Diagnostic {
                        file: path,
                        location: None,
                        problem: Problem::Unreadable {
                            kind: error.kind(),
                            message: error.to_string(),
                        },
                    },
                ),
            })
            .collect();
        Self { files }
    }

    /// The diagnostics of files broken as a whole, lowest layer first.
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.files.iter().filter_map(|file| match &file.contents {
            Contents::Broken(diagnostic) => Some(diagnostic),
            _ => None,
        })
    }
}

fn decode(path: PathBuf, bytes: Vec<u8>) -> File {
    match String::from_utf8(bytes) {
        Ok(text) => parse(path, text),
        Err(error) => {
            let valid = error.utf8_error().valid_up_to();
            let bytes = error.into_bytes();
            // The text up to the first invalid byte is valid, so the
            // location can be counted in it.
            let prefix = String::from_utf8_lossy(&bytes[..valid]).into_owned();
            let file = SourceFile {
                path: path.clone(),
                text: prefix,
            };
            broken(path, Diagnostic::at(&file, valid, Problem::NotUtf8))
        }
    }
}

fn parse(path: PathBuf, text: String) -> File {
    let file = Arc::new(SourceFile {
        path: path.clone(),
        text,
    });
    let parsed = DeTable::parse(&file.text).and_then(|root| Node::from_document(&file, root));
    match parsed {
        Ok(root) => File {
            path,
            contents: Contents::Parsed { file, root },
        },
        Err(error) => {
            let problem = Problem::Syntax {
                message: error.message().trim_end().to_owned(),
            };
            let diagnostic = match error.span() {
                Some(span) => Diagnostic::at(&file, span.start, problem),
                None => Diagnostic {
                    file: path.clone(),
                    location: None,
                    problem,
                },
            };
            broken(path, diagnostic)
        }
    }
}

fn broken(path: PathBuf, diagnostic: Diagnostic) -> File {
    File {
        path,
        contents: Contents::Broken(diagnostic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Location;

    fn only_diagnostic(snapshot: &Snapshot) -> &Diagnostic {
        let all: Vec<_> = snapshot.diagnostics().collect();
        assert_eq!(all.len(), 1, "{all:?}");
        all[0]
    }

    #[test]
    fn reports_syntax_errors_with_their_location() {
        let snapshot = Snapshot::from_texts([(
            PathBuf::from("/c.toml"),
            Some("[panel]\nheight = \n".into()),
        )]);
        let diagnostic = only_diagnostic(&snapshot);
        assert_eq!(diagnostic.file, PathBuf::from("/c.toml"));
        assert_eq!(diagnostic.location.map(|l| l.line), Some(2));
        assert!(matches!(diagnostic.problem, Problem::Syntax { .. }));
    }

    #[test]
    fn reports_duplicate_keys_as_syntax_errors() {
        let snapshot =
            Snapshot::from_texts([("/c.toml".into(), Some("[a]\nx = 1\nx = 2\n".into()))]);
        let diagnostic = only_diagnostic(&snapshot);
        assert_eq!(diagnostic.location.map(|l| l.line), Some(3));
    }

    #[test]
    fn reports_an_out_of_range_integer() {
        let snapshot = Snapshot::from_texts([(
            "/c.toml".into(),
            Some("[a]\nx = 99999999999999999999\n".into()),
        )]);
        let diagnostic = only_diagnostic(&snapshot);
        assert_eq!(diagnostic.location, Some(Location { line: 2, column: 5 }));
    }

    #[test]
    fn reports_unreadable_and_non_utf8_files() {
        let snapshot = Snapshot::from_reads([
            ("/missing.toml".into(), Ok(None)),
            (
                "/denied.toml".into(),
                Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            ),
            (
                "/latin1.toml".into(),
                Ok(Some(b"a = 1\nb = '\xe9'\n".to_vec())),
            ),
        ]);
        let all: Vec<_> = snapshot.diagnostics().collect();
        assert_eq!(all.len(), 2);
        assert!(matches!(
            all[0].problem,
            Problem::Unreadable {
                kind: io::ErrorKind::PermissionDenied,
                ..
            }
        ));
        assert_eq!(all[1].problem, Problem::NotUtf8);
        assert_eq!(all[1].location, Some(Location { line: 2, column: 6 }));
    }
}

//! `cargo xtask file-length`: no Rust source file is longer than
//! [`MAX_LINES`].
//!
//! The rule is "no source file much above 500 lines" (CLAUDE.md); this is its
//! hard stop.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::Outcome;
use crate::error::Error;
use crate::git;

/// The longest a Rust source file may be, in lines.
pub(crate) const MAX_LINES: usize = 600;

/// A file longer than the limit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LongFile {
    /// The file, relative to the repository root.
    path: PathBuf,
    /// How many lines it has.
    lines: usize,
}

impl fmt::Display for LongFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} lines", self.path.display(), self.lines)
    }
}

/// Checks every Rust file tracked by git in `root` and prints the result.
pub(crate) fn run(root: &Path) -> Result<Outcome, Error> {
    let (checked, long) = scan(root, MAX_LINES)?;
    if long.is_empty() {
        println!("file-length: {checked} Rust files, none longer than {MAX_LINES} lines");
        return Ok(Outcome::Passed);
    }
    eprintln!(
        "file-length: {} of {checked} Rust files are longer than {MAX_LINES} lines; \
         split them by responsibility:",
        long.len()
    );
    for file in &long {
        eprintln!("  {file}");
    }
    Ok(Outcome::Failed)
}

/// Counts the lines of every Rust file tracked by git in `root`. Returns how
/// many files were checked and those longer than `max_lines`, in git's order.
fn scan(root: &Path, max_lines: usize) -> Result<(usize, Vec<LongFile>), Error> {
    let mut checked = 0;
    let mut long = Vec::new();
    for path in git::tracked_files(root, "*.rs")? {
        let full = root.join(&path);
        let contents = match fs::read(&full) {
            Ok(contents) => contents,
            // Tracked but deleted in the working tree: whatever is committed
            // next either drops the file or restores it, and the restored
            // copy is checked then.
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(Error::Read { path: full, source }),
        };
        checked += 1;
        if let Some(file) = too_long(path, &contents, max_lines) {
            long.push(file);
        }
    }
    Ok((checked, long))
}

/// The file at `path` with `contents`, if it has more than `max_lines` lines.
fn too_long(path: PathBuf, contents: &[u8], max_lines: usize) -> Option<LongFile> {
    let lines = count_lines(contents);
    (lines > max_lines).then_some(LongFile { path, lines })
}

/// The number of lines in `contents`, as an editor shows them: a last line
/// without a trailing newline still counts.
fn count_lines(contents: &[u8]) -> usize {
    contents.split_inclusive(|&byte| byte == b'\n').count()
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    fn lines(count: usize) -> Vec<u8> {
        "x\n".repeat(count).into_bytes()
    }

    #[test]
    fn counts_lines() {
        assert_eq!(count_lines(b""), 0);
        assert_eq!(count_lines(b"\n"), 1);
        assert_eq!(count_lines(b"one"), 1);
        assert_eq!(count_lines(b"one\ntwo\n"), 2);
        assert_eq!(count_lines(b"one\ntwo"), 2);
        assert_eq!(count_lines(b"one\r\ntwo\r\n"), 2);
        assert_eq!(count_lines(b"\n\n\n"), 3);
    }

    #[test]
    fn the_limit_itself_is_allowed() {
        assert_eq!(too_long("at.rs".into(), &lines(MAX_LINES), MAX_LINES), None);
    }

    #[test]
    fn one_line_over_the_limit_is_too_long() {
        assert_eq!(
            too_long("over.rs".into(), &lines(MAX_LINES + 1), MAX_LINES),
            Some(LongFile {
                path: "over.rs".into(),
                lines: MAX_LINES + 1,
            })
        );
    }

    #[test]
    fn reports_path_and_line_count() {
        let file = LongFile {
            path: "crates/a/src/lib.rs".into(),
            lines: 612,
        };
        assert_eq!(file.to_string(), "crates/a/src/lib.rs: 612 lines");
    }

    /// A scratch git repository holding `files`, all added to the index.
    fn repository(files: &[(&str, &[u8])]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}: {status}");
        };
        git(&["init", "--quiet"]);
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        git(&["add", "--all"]);
        dir
    }

    #[test]
    fn scans_only_tracked_rust_files() {
        let long = lines(4);
        let dir = repository(&[
            ("short.rs", b"fn main() {}\n"),
            ("nested/deep/long.rs", &long),
            ("long.txt", &long),
        ]);
        fs::write(dir.path().join("untracked.rs"), &long).unwrap();

        let (checked, found) = scan(dir.path(), 3).unwrap();

        assert_eq!(checked, 2);
        assert_eq!(
            found,
            [LongFile {
                path: "nested/deep/long.rs".into(),
                lines: 4,
            }]
        );
    }

    #[test]
    fn skips_a_tracked_file_deleted_from_the_working_tree() {
        let dir = repository(&[("gone.rs", &lines(4)), ("kept.rs", b"\n")]);
        fs::remove_file(dir.path().join("gone.rs")).unwrap();

        assert_eq!(scan(dir.path(), 3).unwrap(), (1, Vec::new()));
    }
}

//! What git knows about the repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::Error;

/// The files in `repo`'s index that match `pathspec`, relative to `repo`, in
/// git's order.
pub(crate) fn tracked_files(repo: &Path, pathspec: &str) -> Result<Vec<PathBuf>, Error> {
    let args = ["ls-files", "-z", "--", pathspec];
    let describe = || args.join(" ");
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|source| Error::GitSpawn {
            args: describe(),
            source,
        })?;
    if !output.status.success() {
        return Err(Error::GitFailed {
            args: describe(),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    split_nul_paths(&output.stdout).ok_or_else(|| Error::NonUtf8Path { args: describe() })
}

/// Splits `-z` output (each path followed by a NUL) into paths, or `None` if
/// a path is not UTF-8.
fn split_nul_paths(output: &[u8]) -> Option<Vec<PathBuf>> {
    output
        .split(|&byte| byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| str::from_utf8(path).ok().map(PathBuf::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_nul_terminated_paths() {
        let paths = split_nul_paths(b"a.rs\0dir/with space.rs\0").unwrap();
        assert_eq!(
            paths,
            [PathBuf::from("a.rs"), PathBuf::from("dir/with space.rs")]
        );
    }

    #[test]
    fn empty_output_is_no_paths() {
        assert_eq!(split_nul_paths(b""), Some(Vec::new()));
    }

    #[test]
    fn rejects_a_path_that_is_not_utf8() {
        assert_eq!(split_nul_paths(b"ok.rs\0bad\xff.rs\0"), None);
    }

    #[test]
    fn reports_git_failure_outside_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        let error = tracked_files(dir.path(), "*.rs").unwrap_err();
        assert!(matches!(error, Error::GitFailed { .. }), "{error:?}");
    }
}

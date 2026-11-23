//! The directory a job's packages are fetched into and cached from.
//!
//! It is apt's archives directory for the install (`Dir::Cache::Archives`)
//! in place of `/var/cache/apt/archives`, which only root may write: the
//! cache actions run as the job's user and restore into it. It is owned by
//! the job's user, mode 755, under `/var/cache`, whose every ancestor all
//! may enter, so that `_apt`, the user apt downloads as, reaches the
//! `partial` directory apt makes in it, and downloads stay sandboxed. apt
//! keeps its `lock` and `partial` there too; the cache saves only the
//! `.deb` files.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;
use std::time::Duration;

use super::bounded::{self, User};
use super::classify::Ended;
use deskrinch_test_session::ci::OnARunner;

use super::plan::megabytes;
use crate::error::Error;

/// The directory.
pub(super) const DIRECTORY: &str = "/var/cache/deskrinch-apt";

/// How long making it may take.
pub(super) const PREPARE_LIMIT: Duration = Duration::from_secs(10);

/// Makes [`DIRECTORY`], owned by the job's user, mode 755, with `install
/// -d` as root, and says what it is.
pub(super) fn prepare(runner: OnARunner) -> Result<(), Error> {
    let uid = rustix::process::getuid().as_raw().to_string();
    let gid = rustix::process::getgid().as_raw().to_string();
    let args = ["-d", "-m", "0755", "-o", &uid, "-g", &gid, DIRECTORY].map(String::from);
    let (ended, _) = bounded::run(User::Root(runner), "install", &args, PREPARE_LIMIT)?;
    if ended != Ended::Exited(0) {
        return Err(Error::PackageCache {
            ended: ended.to_string(),
        });
    }
    let metadata = fs::metadata(DIRECTORY).map_err(|source| Error::Io {
        what: format!("read {DIRECTORY}'s owner and mode"),
        source,
    })?;
    println!(
        "Package cache directory {DIRECTORY}: owner uid {}, mode {:o}, for the cache to \
         restore into as the job's user and for apt's _apt user to reach the partial \
         directory apt makes in it.",
        metadata.uid(),
        metadata.permissions().mode() & 0o7777
    );
    Ok(())
}

/// The `.deb` files in `dir`, each with its size in bytes.
pub(super) fn contents(dir: &Path) -> Result<BTreeMap<String, u64>, Error> {
    let io = |source| Error::Io {
        what: format!("list {}", dir.display()),
        source,
    };
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        let metadata = entry.metadata().map_err(io)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if metadata.is_file() && Path::new(&name).extension().is_some_and(|e| e == "deb") {
            files.insert(name, metadata.len());
        }
    }
    Ok(files)
}

/// `files` in a few words: `122 files, 53.3 MB`.
pub(super) fn summary(files: &BTreeMap<String, u64>) -> String {
    format!("{} files, {}", files.len(), megabytes(files.values().sum()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory as apt and a restore leave it: two `.deb` files, apt's
    /// lock and its partial directory with a file half fetched.
    fn archives() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("sway_1.11-1_amd64v3.deb"), [0; 300]).unwrap();
        fs::write(dir.path().join("sway_1.10-2_amd64v3.deb"), [0; 200]).unwrap();
        fs::write(dir.path().join("lock"), []).unwrap();
        fs::create_dir(dir.path().join("partial")).unwrap();
        fs::write(dir.path().join("partial/labwc_0.9_amd64v3.deb"), [0; 50]).unwrap();
        dir
    }

    #[test]
    fn the_contents_are_the_packages_only() {
        let dir = archives();
        let files = contents(dir.path()).unwrap();
        assert_eq!(
            files.into_iter().collect::<Vec<_>>(),
            [
                ("sway_1.10-2_amd64v3.deb".to_owned(), 200),
                ("sway_1.11-1_amd64v3.deb".to_owned(), 300)
            ]
        );
        assert_eq!(summary(&contents(dir.path()).unwrap()), "2 files, 0.0 MB");
    }
}

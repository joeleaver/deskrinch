//! The crate's error type.

use std::error::Error as StdError;
use std::io;
use std::path::PathBuf;

use rustix::io::Errno;

/// A boxed error from the file-watching backend.
pub type BoxedError = Box<dyn StdError + Send + Sync>;

/// Why watching failed, in whole or in part.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// The kernel's limit on inotify watches
    /// (`fs.inotify.max_user_watches`) is reached: `path`, and whatever
    /// else needed a watch after it, is not watched.
    #[error("the inotify watch limit is reached{}", at(path.as_ref()))]
    WatchLimit {
        /// The path that could not be watched, when known.
        path: Option<PathBuf>,
    },
    /// No inotify instance could be created: the limit on instances
    /// (`fs.inotify.max_user_instances`) or on the process's open files
    /// is reached.
    #[error("no inotify instance can be created: too many are open")]
    InstanceLimit,
    /// Any other failure of the file-watching backend.
    #[error("the file-watching backend failed{}", at(path.as_ref()))]
    Backend {
        /// The path concerned, when known.
        path: Option<PathBuf>,
        /// Why.
        #[source]
        source: BoxedError,
    },
    /// The worker thread could not be started.
    #[error("cannot start the watcher thread")]
    Thread(#[source] io::Error),
}

fn at(path: Option<&PathBuf>) -> String {
    path.map(|path| format!(" at {}", path.display()))
        .unwrap_or_default()
}

impl WatchError {
    /// The typed form of the kernel's answer `errno` to watching `path`.
    pub(crate) fn from_errno(errno: Errno, path: Option<PathBuf>) -> Self {
        match errno {
            Errno::NOSPC => Self::WatchLimit { path },
            Errno::MFILE | Errno::NFILE => Self::InstanceLimit,
            _ => Self::Backend {
                path,
                source: Box::new(io::Error::from(errno)),
            },
        }
    }

    /// Whether the error is a path that vanished between planning a watch
    /// and adding it, which the next plan accounts for.
    pub(crate) fn is_path_not_found(&self) -> bool {
        let Self::Backend { source, .. } = self else {
            return false;
        };
        source.downcast_ref::<io::Error>().is_some_and(|error| {
            matches!(
                error.raw_os_error(),
                Some(code) if code == Errno::NOENT.raw_os_error() || code == Errno::NOTDIR.raw_os_error()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_the_inotify_limits() {
        let error = WatchError::from_errno(Errno::NOSPC, Some("/d".into()));
        assert!(matches!(&error, WatchError::WatchLimit { path: Some(p) } if p == "/d"));
        assert_eq!(
            error.to_string(),
            "the inotify watch limit is reached at /d"
        );
        assert!(matches!(
            WatchError::from_errno(Errno::MFILE, None),
            WatchError::InstanceLimit
        ));
        let gone = WatchError::from_errno(Errno::NOENT, Some("/gone".into()));
        assert!(gone.is_path_not_found());
        let other = WatchError::from_errno(Errno::ACCESS, None);
        assert!(matches!(other, WatchError::Backend { path: None, .. }));
        assert!(!other.is_path_not_found());
    }
}

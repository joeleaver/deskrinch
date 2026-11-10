//! The crate's error type.

use std::error::Error as StdError;
use std::io;
use std::path::PathBuf;

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
    /// The typed form of a backend error.
    pub(crate) fn from_backend(error: notify::Error) -> Self {
        let path = error.paths.first().cloned();
        match &error.kind {
            notify::ErrorKind::MaxFilesWatch => Self::WatchLimit { path },
            notify::ErrorKind::Io(io) if io.raw_os_error() == Some(libc::EMFILE) => {
                Self::InstanceLimit
            }
            _ => Self::Backend {
                path,
                source: Box::new(error),
            },
        }
    }

    /// Whether the error is a path that vanished between planning a watch
    /// and adding it, which the next plan accounts for.
    pub(crate) fn is_path_not_found(&self) -> bool {
        let Self::Backend { source, .. } = self else {
            return false;
        };
        source
            .downcast_ref::<notify::Error>()
            .is_some_and(|error| matches!(error.kind, notify::ErrorKind::PathNotFound))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_the_inotify_limits() {
        let full = notify::Error::new(notify::ErrorKind::MaxFilesWatch).add_path("/d".into());
        let error = WatchError::from_backend(full);
        assert!(matches!(&error, WatchError::WatchLimit { path: Some(p) } if p == "/d"));
        assert_eq!(
            error.to_string(),
            "the inotify watch limit is reached at /d"
        );

        let instances = notify::Error::io(io::Error::from_raw_os_error(libc::EMFILE));
        assert!(matches!(
            WatchError::from_backend(instances),
            WatchError::InstanceLimit
        ));

        let gone = WatchError::from_backend(notify::Error::path_not_found());
        assert!(gone.is_path_not_found());
        let other = WatchError::from_backend(notify::Error::generic("broken"));
        assert!(matches!(other, WatchError::Backend { path: None, .. }));
        assert!(!other.is_path_not_found());
    }
}

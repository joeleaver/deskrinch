//! [`Store`]: the record's file, read, changed under a lock, and written
//! whole through a rename.

use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use deskrinch_desktop_entries::DesktopId;
use deskrinch_xdg::BaseDirs;

use crate::format::{self, Parsed};
use crate::record::Record;

/// The directory under `XDG_STATE_HOME` the record is kept in.
pub const DIRECTORY: &str = "deskrinch";
/// The record's file in [`DIRECTORY`].
pub const FILE: &str = "starts.toml";
/// The lock file beside it, which every change holds.
pub const LOCK: &str = "starts.lock";
/// What a new file's name starts with until it is renamed over [`FILE`].
const PENDING: &str = ".starts.toml.";

/// The mode the directory is made with: the user's alone.
const DIRECTORY_MODE: u32 = 0o700;
/// The mode the record and its lock are made with: the user's alone.
const FILE_MODE: u32 = 0o600;

/// The record's file and the lock beside it, in one directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    directory: PathBuf,
}

/// What [`Store::load`] found.
#[derive(Debug, Clone, PartialEq)]
pub enum Loaded {
    /// No file: no start is recorded.
    Missing,
    /// The record, with the number of starts left out as invalid.
    Read {
        /// The record.
        record: Record,
        /// How many `[[start]]` tables were not valid, and left out.
        dropped: usize,
    },
    /// A file that is not a record of starts (not TOML, or not this
    /// shape): no start is recorded, and the next change writes a record
    /// over it.
    Damaged {
        /// What is wrong with it.
        reason: String,
    },
    /// A record of a newer version: it is left as it is, and every change
    /// is refused ([`StoreError::Newer`]).
    Newer {
        /// Its version.
        version: i64,
    },
}

impl Loaded {
    /// The record found: empty unless one was read.
    #[must_use]
    pub fn into_record(self) -> Record {
        match self {
            Self::Read { record, .. } => record,
            Self::Missing | Self::Damaged { .. } | Self::Newer { .. } => Record::new(),
        }
    }
}

/// Why the record could not be read or changed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The file or its directory could not be read.
    #[error("cannot read {}", path.display())]
    Read {
        /// What could not be read.
        path: PathBuf,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// The file, its lock or its directory could not be written.
    #[error("cannot write {}", path.display())]
    Write {
        /// What could not be written.
        path: PathBuf,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// The file is a newer version's, and is left as it is.
    #[error("{} is a newer version's record (version {version}), and is left as it is", path.display())]
    Newer {
        /// The file.
        path: PathBuf,
        /// Its version.
        version: i64,
    },
    /// The record could not be written as TOML.
    #[error("cannot write the record as TOML")]
    Format(#[source] toml::ser::Error),
}

impl Store {
    /// The record in the user's state directory,
    /// `$XDG_STATE_HOME/deskrinch/`; `None` when there is none (neither
    /// `XDG_STATE_HOME` nor `HOME` is set).
    #[must_use]
    pub fn new(dirs: &BaseDirs) -> Option<Self> {
        let state = dirs.state_home.as_ref()?;
        Some(Self::in_directory(state.join(DIRECTORY)))
    }

    /// The record in `directory`.
    #[must_use]
    pub fn in_directory(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// The record's file.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.directory.join(FILE)
    }

    /// Reads the record as the file now has it. It takes no lock: the file
    /// is only ever replaced whole, so a read sees one version or another.
    /// It is also how a holder of the record catches up with the file after
    /// another writer changed it.
    ///
    /// # Errors
    ///
    /// [`StoreError::Read`] when the file exists and cannot be read.
    pub fn load(&self) -> Result<Loaded, StoreError> {
        let path = self.path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Loaded::Missing),
            Err(source) => return Err(StoreError::Read { path, source }),
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return Ok(Loaded::Damaged {
                reason: "it is not UTF-8 text".to_owned(),
            });
        };
        Ok(match format::parse(&text) {
            Parsed::Record { record, dropped } => Loaded::Read { record, dropped },
            Parsed::Damaged(reason) => Loaded::Damaged { reason },
            Parsed::Newer(version) => Loaded::Newer { version },
        })
    }

    /// Records a start of `id` at `now` (seconds since the Unix epoch):
    /// under the lock, the file is read again, the start added, the record
    /// pruned, and the whole written back. A damaged file is replaced, and
    /// that is logged. Answers the record as written.
    ///
    /// The directory is made (mode 0700) if it is missing; the file and the
    /// lock are the user's alone (0600).
    ///
    /// # Errors
    ///
    /// [`StoreError::Newer`] when the file is a newer version's, which is
    /// left as it is; [`StoreError::Read`], [`StoreError::Write`] and
    /// [`StoreError::Format`] when the file cannot be read or written.
    pub fn record(&self, id: &DesktopId, now: u64) -> Result<Record, StoreError> {
        let _lock = self.lock()?;
        let mut record = match self.load()? {
            Loaded::Missing => Record::new(),
            Loaded::Read { record, .. } => record,
            Loaded::Damaged { reason } => {
                tracing::warn!(
                    "replacing {}, which is not a record of starts: {reason}",
                    self.path().display()
                );
                Record::new()
            }
            Loaded::Newer { version } => {
                return Err(StoreError::Newer {
                    path: self.path(),
                    version,
                });
            }
        };
        record.record(id, now);
        self.write(&record)?;
        Ok(record)
    }

    /// Deletes the record, under the lock; answers whether there was one.
    /// With no directory there is nothing to delete, and nothing is made.
    /// The lock file stays: a writer may be waiting on it.
    ///
    /// # Errors
    ///
    /// [`StoreError::Write`] when the file cannot be deleted.
    pub fn forget(&self) -> Result<bool, StoreError> {
        if !self.directory.is_dir() {
            return Ok(false);
        }
        let _lock = self.lock()?;
        let path = self.path();
        let deleted = match fs::remove_file(&path) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(source) => return Err(StoreError::Write { path, source }),
        };
        self.remove_pending()?;
        self.sync_directory()?;
        Ok(deleted)
    }

    /// Takes the lock, making the directory and the lock file if they are
    /// missing; it is held until the file is dropped.
    fn lock(&self) -> Result<File, StoreError> {
        let write = |path: &Path| {
            let path = path.to_owned();
            move |source| StoreError::Write { path, source }
        };
        DirBuilder::new()
            .recursive(true)
            .mode(DIRECTORY_MODE)
            .create(&self.directory)
            .map_err(write(&self.directory))?;
        let path = self.directory.join(LOCK);
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(FILE_MODE)
            .open(&path)
            .map_err(write(&path))?;
        lock.lock().map_err(write(&path))?;
        Ok(lock)
    }

    /// Writes `record` whole: to a new file beside the old, made the
    /// user's alone and synced, then renamed over it, and the directory
    /// synced. A crash leaves the old file or the new, never a part of
    /// either. Called under the lock.
    fn write(&self, record: &Record) -> Result<(), StoreError> {
        let text = format::write(record).map_err(StoreError::Format)?;
        self.remove_pending()?;
        let path = self.path();
        let failed = |source| StoreError::Write {
            path: path.clone(),
            source,
        };
        let mut new = tempfile::Builder::new()
            .prefix(PENDING)
            .tempfile_in(&self.directory)
            .map_err(failed)?;
        new.as_file()
            .set_permissions(Permissions::from_mode(FILE_MODE))
            .map_err(failed)?;
        new.write_all(text.as_bytes()).map_err(failed)?;
        new.as_file().sync_all().map_err(failed)?;
        new.persist(&path).map_err(|error| failed(error.error))?;
        self.sync_directory()
    }

    /// Removes new files a writer that crashed left before renaming them.
    /// Called under the lock, when no other writer can be writing one.
    fn remove_pending(&self) -> Result<(), StoreError> {
        let read = |source| StoreError::Read {
            path: self.directory.clone(),
            source,
        };
        for entry in fs::read_dir(&self.directory).map_err(read)? {
            let entry = entry.map_err(read)?;
            let pending = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(PENDING));
            if pending {
                let path = entry.path();
                fs::remove_file(&path).map_err(|source| StoreError::Write { path, source })?;
            }
        }
        Ok(())
    }

    /// Syncs the directory, so that a rename or a removal in it lasts.
    fn sync_directory(&self) -> Result<(), StoreError> {
        let write = |source| StoreError::Write {
            path: self.directory.clone(),
            source,
        };
        File::open(&self.directory)
            .map_err(write)?
            .sync_all()
            .map_err(write)
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

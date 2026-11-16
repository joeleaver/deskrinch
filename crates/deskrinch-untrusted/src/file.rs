//! A file a stranger named, read under a bound: what a peer may point the
//! shell at (a notification's `image-path`, say) without making it block,
//! read a device, or fill memory.
//!
//! [`read_bounded`] opens the path without blocking (`O_NONBLOCK`, so a FIFO
//! with no writer does not hang the open, and `O_NOCTTY`), asks the opened
//! descriptor what it is (`fstat`, so the answer is about the file read and
//! not about whatever the path named a moment earlier), refuses anything but
//! a regular file, and reads at most the bound and one byte more from that
//! descriptor: a file that grows after the check is still refused, not read
//! whole.

use std::fs::{File, OpenOptions};
use std::io::{self, Read as _};
use std::os::unix::fs::{FileTypeExt as _, OpenOptionsExt as _};
use std::path::Path;

/// Why a named file was not read.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// It could not be opened or read.
    #[error("cannot read it: {0}")]
    Io(#[from] io::Error),
    /// It is a directory.
    #[error("it is a directory")]
    Directory,
    /// It is a FIFO (a named pipe).
    #[error("it is a named pipe")]
    Fifo,
    /// It is a character or block device.
    #[error("it is a device")]
    Device,
    /// It is something else that is not a regular file. (A socket cannot
    /// be opened at all: its open fails, as [`FileError::Io`].)
    #[error("it is not a regular file")]
    NotRegular,
    /// It holds more than the bound allows: `size` is at least what it
    /// holds (the bound and one byte more, when it grew while it was read).
    #[error("it holds {size} bytes, more than {max}")]
    TooLarge {
        /// Its size, or at least the bound and one byte more.
        size: u64,
        /// The bound.
        max: usize,
    },
}

/// The bytes of the regular file at `path`, at most `max` of them.
///
/// # Errors
///
/// [`FileError`]: it cannot be opened or read, it is not a regular file, or
/// it holds more than `max` bytes.
pub fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>, FileError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)?;
    let size = check_regular(&file)?;
    let bound = u64::try_from(max).unwrap_or(u64::MAX);
    if size > bound {
        return Err(FileError::TooLarge { size, max });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(max));
    file.take(bound.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(FileError::TooLarge {
            size: bound.saturating_add(1),
            max,
        });
    }
    Ok(bytes)
}

/// The size of the opened `file`, if it is a regular file.
fn check_regular(file: &File) -> Result<u64, FileError> {
    let metadata = file.metadata()?;
    let kind = metadata.file_type();
    if kind.is_file() {
        Ok(metadata.len())
    } else if kind.is_dir() {
        Err(FileError::Directory)
    } else if kind.is_fifo() {
        Err(FileError::Fifo)
    } else if kind.is_char_device() || kind.is_block_device() {
        Err(FileError::Device)
    } else {
        Err(FileError::NotRegular)
    }
}

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;

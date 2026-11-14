//! Listing the icons directly in a directory, without an allocation per
//! entry: `getdents` into one buffer, reused for every directory of a
//! build, each name read where it lies.
//!
//! What is listed is what `std::fs::read_dir` with `DirEntry::file_type`
//! listed before: a regular file, or a symbolic link of any kind (to a
//! file, to a directory, or dangling), without following it; named in
//! UTF-8 with an icon format's extension and a stem before it (so a hidden
//! `.name.png` is the icon `.name`, and `.png` is none). `.` and `..` are
//! not entries. Where the file system gives no type in the listing
//! (`DT_UNKNOWN`), the entry's own `lstat` says it, as std's `file_type`
//! does. An error reading the directory part way ends its listing, as
//! std's iterator ended.

use std::io;
use std::path::Path;

use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags, RawDir, openat, statat};

use crate::index::Format;

/// The listing buffer's size: room for a hundred or more entries per
/// `getdents`, and for the longest one a file system allows.
const BUFFER: usize = 32 * 1024;

/// Lists directories through one buffer, allocated at the first listing.
#[derive(Debug)]
pub(crate) struct Lister {
    buffer: Vec<u8>,
}

impl Lister {
    pub(crate) fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Calls `each` with the stem and format of every icon directly in
    /// `dir` (see the module's description), in the directory's order.
    ///
    /// # Errors
    ///
    /// When `dir` cannot be opened as a directory: an `io::Error` as
    /// `std::fs::read_dir` gives it (`NotFound` for a missing directory).
    pub(crate) fn list(&mut self, dir: &Path, mut each: impl FnMut(&str, Format)) -> io::Result<()> {
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
        let fd = openat(CWD, dir, flags, Mode::empty())?;
        self.buffer.reserve_exact(BUFFER);
        let mut entries = RawDir::new(&fd, self.buffer.spare_capacity_mut());
        while let Some(entry) = entries.next() {
            let Ok(entry) = entry else {
                break;
            };
            let Ok(name) = entry.file_name().to_str() else {
                continue;
            };
            if name == "." || name == ".." {
                continue;
            }
            let Some((stem, format)) = Format::split(name) else {
                continue;
            };
            let lstat = || {
                let stat = statat(&fd, entry.file_name(), AtFlags::SYMLINK_NOFOLLOW)?;
                Ok(FileType::from_raw_mode(stat.st_mode))
            };
            if listed(entry.file_type(), lstat) {
                each(stem, format);
            }
        }
        Ok(())
    }
}

/// Whether an entry of `file_type` is listed: a regular file or a link.
/// An unknown type is `lstat`'s; an entry `lstat` fails on is not listed.
fn listed(file_type: FileType, lstat: impl FnOnce() -> rustix::io::Result<FileType>) -> bool {
    let file_type = match file_type {
        FileType::Unknown => match lstat() {
            Ok(file_type) => file_type,
            Err(_) => return false,
        },
        known => known,
    };
    matches!(file_type, FileType::RegularFile | FileType::Symlink)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_and_links_are_listed_and_nothing_else() {
        let unasked = || -> rustix::io::Result<FileType> { panic!("a known type needs no lstat") };
        assert!(listed(FileType::RegularFile, unasked));
        assert!(listed(FileType::Symlink, unasked));
        for other in [
            FileType::Directory,
            FileType::Fifo,
            FileType::Socket,
            FileType::CharacterDevice,
            FileType::BlockDevice,
        ] {
            assert!(!listed(other, unasked), "{other:?}");
        }
    }

    #[test]
    fn an_unknown_type_is_lstats() {
        assert!(listed(FileType::Unknown, || Ok(FileType::RegularFile)));
        assert!(listed(FileType::Unknown, || Ok(FileType::Symlink)));
        assert!(!listed(FileType::Unknown, || Ok(FileType::Directory)));
        assert!(!listed(FileType::Unknown, || Err(rustix::io::Errno::NOENT)));
    }
}

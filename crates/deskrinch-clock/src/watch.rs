//! Watching the time zone file with inotify.
//!
//! `timedatectl set-timezone` replaces `/etc/localtime`, a symbolic link,
//! by renaming a new link over it; other tools delete and recreate it, or
//! copy a file over it. None of that touches the old link's inode, so the
//! file's directory is watched, for entries of the file's name being
//! created, moved in, deleted or written.
//!
//! A tzdata update instead rewrites the file the link points to. So the
//! file at the end of the link is watched too, for being written, renamed,
//! deleted or losing a link (a rename over it), and it is watched afresh
//! each time the zone is read, since the link may point elsewhere by then.

use std::ffi::OsString;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

use rustix::fs::inotify::{self, CreateFlags, Reader, WatchFlags};
use rustix::io::Errno;

/// The inotify watches on one time zone file.
#[derive(Debug)]
pub(crate) struct ZoneWatch {
    fd: OwnedFd,
    /// The watch on the file's directory, and the file's name in it.
    dir: Option<(i32, OsString)>,
    /// The watch on the file the path resolves to.
    target: Option<i32>,
}

impl ZoneWatch {
    pub(crate) fn new() -> io::Result<Self> {
        let fd = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK)?;
        Ok(Self {
            fd,
            dir: None,
            target: None,
        })
    }

    /// Watches `file` (or nothing), replacing the previous watches.
    pub(crate) fn watch(&mut self, file: Option<&Path>) -> io::Result<()> {
        let (dir, target) = match file {
            Some(file) => (self.watch_dir(file)?, self.watch_target(file)),
            None => (None, None),
        };
        for (old, new) in [
            (
                self.dir.as_ref().map(|(wd, _)| *wd),
                dir.as_ref().map(|(wd, _)| *wd),
            ),
            (self.target, target),
        ] {
            if let Some(old) = old.filter(|old| Some(*old) != new) {
                // The kernel drops a watch whose inode is gone, so the
                // watch may not exist any more.
                inotify::remove_watch(&self.fd, old).ok();
            }
        }
        self.dir = dir;
        self.target = target;
        Ok(())
    }

    fn watch_dir(&self, file: &Path) -> io::Result<Option<(i32, OsString)>> {
        let (Some(dir), Some(name)) = (file.parent(), file.file_name()) else {
            return Ok(None);
        };
        let flags = WatchFlags::CREATE
            | WatchFlags::MOVED_TO
            | WatchFlags::MOVED_FROM
            | WatchFlags::DELETE
            | WatchFlags::CLOSE_WRITE
            | WatchFlags::ONLYDIR;
        let wd = inotify::add_watch(&self.fd, dir, flags)?;
        Ok(Some((wd, name.to_owned())))
    }

    /// The watch on what `file` resolves to, or `None` while it does not
    /// exist (the directory watch notices it appearing).
    fn watch_target(&self, file: &Path) -> Option<i32> {
        let flags = WatchFlags::CLOSE_WRITE
            | WatchFlags::ATTRIB
            | WatchFlags::DELETE_SELF
            | WatchFlags::MOVE_SELF;
        inotify::add_watch(&self.fd, file, flags).ok()
    }

    /// Reads every pending event, and returns whether any concerns the
    /// file.
    pub(crate) fn changed(&self) -> io::Result<bool> {
        let mut buffer = [MaybeUninit::<u8>::uninit(); 4096];
        let mut reader = Reader::new(&self.fd, &mut buffer);
        let mut changed = false;
        loop {
            match reader.next() {
                Ok(event) => changed |= self.concerns(event.wd(), event.file_name()),
                Err(Errno::AGAIN) => return Ok(changed),
                Err(Errno::INTR) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn concerns(&self, wd: i32, name: Option<&std::ffi::CStr>) -> bool {
        if Some(wd) == self.target {
            return true;
        }
        match &self.dir {
            Some((dir, file)) if *dir == wd => {
                name.is_some_and(|name| name.to_bytes() == file.as_bytes())
            }
            _ => false,
        }
    }
}

impl AsFd for ZoneWatch {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn notices_the_link_replaced_and_the_target_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let (berlin, tokyo) = (dir.path().join("berlin"), dir.path().join("tokyo"));
        fs::write(&berlin, "b").unwrap();
        fs::write(&tokyo, "t").unwrap();
        let localtime = dir.path().join("localtime");
        symlink(&berlin, &localtime).unwrap();

        let mut watch = ZoneWatch::new().unwrap();
        watch.watch(Some(&localtime)).unwrap();
        assert!(!watch.changed().unwrap());

        // Another file in the directory is not the zone.
        fs::write(dir.path().join("unrelated"), "x").unwrap();
        assert!(!watch.changed().unwrap());

        // timedatectl's way: a new link renamed over the old.
        let new_link = dir.path().join(".localtime.new");
        symlink(&tokyo, &new_link).unwrap();
        fs::rename(&new_link, &localtime).unwrap();
        assert!(watch.changed().unwrap());
        watch.watch(Some(&localtime)).unwrap();

        // A tzdata update rewriting the target.
        fs::write(&tokyo, "t2").unwrap();
        assert!(watch.changed().unwrap());
        // The old target is no longer watched.
        fs::write(&berlin, "b2").unwrap();
        assert!(!watch.changed().unwrap());
    }

    #[test]
    fn notices_a_missing_file_appearing() {
        let dir = tempfile::tempdir().unwrap();
        let localtime = dir.path().join("localtime");
        let mut watch = ZoneWatch::new().unwrap();
        watch.watch(Some(&localtime)).unwrap();
        fs::write(&localtime, "x").unwrap();
        assert!(watch.changed().unwrap());
    }
}

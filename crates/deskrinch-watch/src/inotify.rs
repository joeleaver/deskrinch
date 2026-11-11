//! The kernel's notice of changes: inotify watches on the directories a
//! [`Plan`](crate::plan::Plan) names, and an eventfd that wakes the waiting
//! thread for a message. Both through `rustix`, safely.
//!
//! Each directory is asked for exactly the events that can change what a
//! watcher's users read, and nothing else: no `OPEN`, `ACCESS` or
//! `CLOSE_NOWRITE` anywhere, so reading a watched file, or listing a
//! watched directory, wakes nothing.
//!
//! - A directory whose entries' contents matter ([`How::entries`]): an
//!   entry made, removed, renamed in or out, written and closed, or its
//!   attributes (its mode, its owner) changed.
//! - A directory watched only for names (the one holding a tree, the
//!   nearest existing ancestor of a missing path): an entry made, removed
//!   or renamed.
//! - Every watched directory: its own removal or move.
//!
//! A recursive watch is one inotify watch per directory of the tree,
//! symbolic links to directories followed (each directory once). A
//! directory made or moved into the tree is watched as soon as its event is
//! read, before the change is reported, so the subscriber's read after the
//! report sees what was written into it in between.
//!
//! Two paths to one directory share one inotify descriptor, and adding a
//! watch replaces its events: so a descriptor is always given the union of
//! what every path watching it wants. A queue overflow, and a watch the
//! kernel dropped (its directory gone), are a change of unknown place. An
//! error from `poll` or `read` is [`Wait::Failed`]: it would fail again at
//! once, so the watcher stops rather than spin.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
use rustix::fd::OwnedFd;
use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};
use rustix::io::Errno;

use crate::error::WatchError;
use crate::worker::{Backend, Change, How, Wait, Waker};

/// What every watched directory is watched for about itself.
const ITSELF: WatchFlags = WatchFlags::DELETE_SELF
    .union(WatchFlags::MOVE_SELF)
    .union(WatchFlags::ONLYDIR);
/// Names made, removed and renamed.
const NAMES: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::DELETE)
    .union(WatchFlags::MOVED_FROM)
    .union(WatchFlags::MOVED_TO);
/// Names, and the entries written or their attributes changed.
const ENTRIES: WatchFlags = NAMES
    .union(WatchFlags::CLOSE_WRITE)
    .union(WatchFlags::ATTRIB);

/// The events `how` asks for.
fn flags(how: How) -> WatchFlags {
    ITSELF
        | if how.entries || how.recursive {
            ENTRIES
        } else {
            NAMES
        }
}

/// One directory a watch was asked for, under one name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Part {
    /// The directory asked for ([`Backend::watch`]'s `dir`).
    root: PathBuf,
    /// This directory, as named under `root`.
    path: PathBuf,
    flags: WatchFlags,
    /// Whether directories made in it are watched too.
    recursive: bool,
}

/// The inotify backend.
#[derive(Debug)]
pub(crate) struct Inotify {
    fd: OwnedFd,
    wake: Arc<OwnedFd>,
    /// What each descriptor is watched for, by whom.
    parts: HashMap<i32, Vec<Part>>,
    /// Descriptors this backend removed whose `IGNORED` is still to come.
    removed: HashSet<i32>,
    buf: Vec<MaybeUninit<u8>>,
}

impl Inotify {
    pub(crate) fn new() -> Result<Self, WatchError> {
        let instance = |errno| WatchError::from_errno(errno, None);
        let fd = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK).map_err(instance)?;
        let wake = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).map_err(instance)?;
        Ok(Self {
            fd,
            wake: Arc::new(wake),
            parts: HashMap::new(),
            removed: HashSet::new(),
            buf: vec![MaybeUninit::uninit(); 16 * 1024],
        })
    }

    /// Watches `path`, a directory named under `root`, for `flags`, sharing
    /// a descriptor already watched under another name.
    fn add(
        &mut self,
        root: &Path,
        path: &Path,
        flags: WatchFlags,
        recursive: bool,
    ) -> Result<(), Errno> {
        let mut wd = inotify::add_watch(&self.fd, path, flags)?;
        if let Some(before) = self.parts.get(&wd).map(|parts| union(parts))
            && !flags.contains(before)
        {
            // The add replaced the events an earlier name wanted: give it
            // all of them.
            wd = inotify::add_watch(&self.fd, path, before | flags)?;
        }
        self.removed.remove(&wd);
        let part = Part {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
            flags,
            recursive,
        };
        let parts = self.parts.entry(wd).or_default();
        if !parts.contains(&part) {
            parts.push(part);
        }
        Ok(())
    }

    /// Watches `dir`, and with `recursive` every directory below it, for
    /// `root`. The first directory that could not be watched for the limit
    /// is the error; others that vanished or cannot be read are skipped.
    fn add_tree(&mut self, root: &Path, dir: &Path, how: How) -> Result<(), WatchError> {
        let dirs = if how.recursive {
            directories(dir)
        } else {
            vec![dir.to_path_buf()]
        };
        let mut first_error = None;
        for (index, path) in dirs.iter().enumerate() {
            match self.add(root, path, flags(how), how.recursive) {
                // Watched; or gone or unreadable since the walk, and not
                // watched.
                Ok(()) | Err(Errno::NOENT | Errno::NOTDIR | Errno::ACCESS) => {}
                // The tree's own directory: the error is the caller's.
                Err(errno) if index == 0 => {
                    return Err(WatchError::from_errno(errno, Some(path.clone())));
                }
                Err(Errno::NOSPC) => {
                    first_error.get_or_insert(WatchError::WatchLimit {
                        path: Some(path.clone()),
                    });
                }
                Err(errno) => {
                    first_error.get_or_insert(WatchError::from_errno(errno, Some(path.clone())));
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Reads every queued event.
    fn read_events(&mut self) -> Wait {
        let mut changes = Vec::new();
        let mut reader = inotify::Reader::new(&self.fd, &mut self.buf);
        let mut new_dirs: Vec<(PathBuf, PathBuf)> = Vec::new();
        loop {
            let event = match reader.next() {
                Ok(event) => event,
                Err(Errno::AGAIN) => break,
                Err(errno) => return Wait::Failed(WatchError::from_errno(errno, None)),
            };
            let flags = event.events();
            let wd = event.wd();
            if flags.contains(ReadFlags::QUEUE_OVERFLOW) {
                changes.push(Change::Unknown);
                continue;
            }
            if flags.contains(ReadFlags::IGNORED) {
                // Ours if this backend removed it; else the kernel dropped
                // it, the directory gone.
                if !self.removed.remove(&wd) && self.parts.remove(&wd).is_some() {
                    changes.push(Change::Unknown);
                }
                continue;
            }
            let Some(parts) = self.parts.get(&wd) else {
                continue;
            };
            let name = event
                .file_name()
                .map(|name| OsStr::from_bytes(name.to_bytes()));
            for part in parts {
                let path = name.map_or_else(|| part.path.clone(), |name| part.path.join(name));
                let made_dir = flags.contains(ReadFlags::ISDIR)
                    && flags.intersects(ReadFlags::CREATE | ReadFlags::MOVED_TO);
                if part.recursive && made_dir {
                    new_dirs.push((part.root.clone(), path.clone()));
                }
                changes.push(Change::At(path));
            }
        }
        for (root, dir) in new_dirs {
            let how = How {
                recursive: true,
                entries: true,
            };
            if let Err(error) = self.add_tree(&root, &dir, how)
                && !error.is_path_not_found()
            {
                changes.push(Change::Error(error));
            }
        }
        if changes.is_empty() {
            Wait::Quiet
        } else {
            Wait::Changes(changes)
        }
    }
}

impl Backend for Inotify {
    fn watch(&mut self, dir: &Path, how: How) -> Result<(), WatchError> {
        self.add_tree(dir, dir, how)
    }

    fn unwatch(&mut self, dir: &Path) {
        let wds: Vec<i32> = self.parts.keys().copied().collect();
        for wd in wds {
            let Some(parts) = self.parts.get_mut(&wd) else {
                continue;
            };
            let before = union(parts);
            parts.retain(|part| part.root != dir);
            if parts.is_empty() {
                self.parts.remove(&wd);
                // The kernel drops a watch whose directory is gone, so it
                // may not exist any more.
                if inotify::remove_watch(&self.fd, wd).is_ok() {
                    self.removed.insert(wd);
                }
            } else if union(parts) != before {
                // Fewer events wanted: the remaining names' union only.
                let path = parts[0].path.clone();
                let flags = union(parts);
                inotify::add_watch(&self.fd, &path, flags).ok();
            }
        }
    }

    fn wait(&mut self, timeout: Option<Duration>) -> Wait {
        let timeout = timeout.map(|t| Timespec {
            tv_sec: t.as_secs().try_into().unwrap_or(i64::MAX),
            tv_nsec: t.subsec_nanos().into(),
        });
        let mut fds = [
            PollFd::new(&self.fd, PollFlags::IN),
            PollFd::new(&*self.wake, PollFlags::IN),
        ];
        match poll(&mut fds, timeout.as_ref()) {
            Ok(0) | Err(Errno::INTR) => return Wait::Quiet,
            Ok(_) => {}
            Err(errno) => return Wait::Failed(WatchError::from_errno(errno, None)),
        }
        let (events, woken) = (fds[0].revents(), fds[1].revents());
        if !woken.is_empty() {
            let mut count = [0_u8; 8];
            // Non-blocking, and readable: it holds a count.
            rustix::io::read(&*self.wake, &mut count).ok();
            return Wait::Woken;
        }
        if events.is_empty() {
            return Wait::Quiet;
        }
        self.read_events()
    }

    fn waker(&self) -> Waker {
        let wake = Arc::clone(&self.wake);
        Arc::new(move || {
            // An eventfd counter: a write fails only short of 2^64 - 1
            // pending notices, which then wake the thread anyway.
            rustix::io::write(&*wake, &1_u64.to_ne_bytes()).ok();
        })
    }
}

/// The events every name of one descriptor wants.
fn union(parts: &[Part]) -> WatchFlags {
    parts
        .iter()
        .fold(WatchFlags::empty(), |all, part| all | part.flags)
}

/// `dir` and every directory below it, through symbolic links, each
/// directory once (a link back up the tree ends there), in path order.
fn directories(dir: &Path) -> Vec<PathBuf> {
    let mut found = BTreeSet::new();
    let mut seen = HashSet::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(real) = next.canonicalize() else {
            continue;
        };
        if !seen.insert(real) {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&next) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                }
            }
        }
        found.insert(next);
    }
    found.into_iter().collect()
}

#[cfg(test)]
mod tests;

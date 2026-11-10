//! Watching the configuration files and delivering a section's new
//! settings.
//!
//! The watcher runs on a thread of its own. It watches the directory of
//! every layer's file (not the file, so that an editor saving by writing a
//! new file and renaming it over the old one is seen), or, when the
//! directory does not exist yet, its nearest existing ancestor (so its
//! creation is seen). A file that is a symbolic link is also watched where
//! it points. After a burst of changes to those files settles, it reads
//! them all again and resolves the section; what changed is delivered.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::diagnostic::Diagnostic;
use crate::error::WatchError;
use crate::section::{Resolved, Section, Settings};
use crate::snapshot::Snapshot;
use crate::sources::Sources;

/// How long changes must pause before the files are read again.
const QUIET: Duration = Duration::from_millis(100);
/// The longest a reload waits during a continuous stream of changes.
const MAX_DELAY: Duration = Duration::from_secs(1);

/// What a reload found.
#[derive(Debug)]
pub enum Event<T> {
    /// The settings changed; these are the new ones.
    Changed(T),
    /// The diagnostics changed (a file broke, was fixed, or gained or lost
    /// an unknown key); these are all of them now, and empty when nothing
    /// is wrong any more. Delivered before the [`Changed`](Self::Changed)
    /// of the same reload.
    Diagnostics(Vec<Diagnostic>),
    /// Watching failed in part; the watcher carries on.
    Error(WatchError),
}

/// Watches the configuration files and reports a section's [`Event`]s to a
/// subscriber until dropped.
#[derive(Debug)]
pub struct Watcher {
    messages: Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

enum Message {
    Changed(notify::Result<notify::Event>),
    Stop,
}

impl Watcher {
    /// Starts watching the files of `sources` for the section `name`, and
    /// returns the settings the reported events are changes to.
    /// `subscriber` is called on the watcher's thread, one event at a time.
    ///
    /// A reload that leaves the settings as they were delivers no
    /// [`Event::Changed`]; one that leaves the diagnostics as they were
    /// delivers no [`Event::Diagnostics`].
    ///
    /// # Errors
    ///
    /// [`WatchError`] when `T`'s defaults are unusable, or the
    /// file-watching backend or the thread cannot be started, or a
    /// directory cannot be watched (for example when the inotify watch
    /// limit is reached).
    pub fn start<T: Settings>(
        sources: Sources,
        name: &str,
        subscriber: impl Fn(Event<T>) + Send + 'static,
    ) -> Result<(Self, Resolved<T>), WatchError> {
        let section = Section::new(name)?;
        let (messages, inbox) = mpsc::channel();
        let forward = messages.clone();
        let backend = notify::recommended_watcher(move |change| {
            // A send fails only once the worker has stopped, when there is
            // no one left to tell.
            forward.send(Message::Changed(change)).ok();
        })
        .map_err(|error| WatchError::Backend(Box::new(error)))?;
        let mut worker = Worker {
            backend,
            targets: Targets::default(),
            sources,
            section,
            settings: T::default(),
            diagnostics: Vec::new(),
            subscriber: Box::new(subscriber),
        };
        // Watch first, then read: a change between the two is reported
        // rather than lost.
        if let Some(error) = worker.retarget().into_iter().next() {
            return Err(error);
        }
        let resolved = worker.section.resolve(&Snapshot::read(&worker.sources));
        worker.settings.clone_from(&resolved.settings);
        worker.diagnostics.clone_from(&resolved.diagnostics);
        let handle = thread::Builder::new()
            .name("config-watch".into())
            .spawn(move || worker.run(&inbox))
            .map_err(WatchError::Thread)?;
        Ok((
            Self {
                messages,
                worker: Some(handle),
            },
            resolved,
        ))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.messages.send(Message::Stop).ok();
        if let Some(worker) = self.worker.take() {
            // A panic in the subscriber ended the thread already; there is
            // nothing more to stop.
            worker.join().ok();
        }
    }
}

struct Worker<T> {
    backend: RecommendedWatcher,
    targets: Targets,
    sources: Sources,
    section: Section<T>,
    /// The settings and diagnostics last delivered.
    settings: T,
    diagnostics: Vec<Diagnostic>,
    subscriber: Box<dyn Fn(Event<T>) + Send>,
}

impl<T: Settings> Worker<T> {
    fn run(mut self, inbox: &Receiver<Message>) {
        while let Ok(Message::Changed(change)) = inbox.recv() {
            let mut relevant = self.consider(change);
            let deadline = Instant::now() + MAX_DELAY;
            loop {
                let wait = QUIET.min(deadline.saturating_duration_since(Instant::now()));
                match inbox.recv_timeout(wait) {
                    Ok(Message::Changed(change)) => relevant |= self.consider(change),
                    Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => break,
                }
            }
            if relevant {
                for error in self.retarget() {
                    (self.subscriber)(Event::Error(error));
                }
                self.reload();
            }
        }
    }

    /// Whether a change may concern a layer's file. Reading the files (as
    /// a reload does) changes nothing.
    fn consider(&self, change: notify::Result<notify::Event>) -> bool {
        match change {
            Ok(event) => {
                !matches!(event.kind, EventKind::Access(_))
                    && event.paths.iter().any(|path| self.targets.concerns(path))
            }
            Err(error) => {
                (self.subscriber)(Event::Error(WatchError::Backend(Box::new(error))));
                true
            }
        }
    }

    /// Watches what the files now need, and returns the watches that could
    /// not be added.
    fn retarget(&mut self) -> Vec<WatchError> {
        let wanted = Targets::for_files(self.sources.files());
        for path in self.targets.watched.difference(&wanted.watched) {
            // The directory may be gone already, which removed its watch.
            self.backend.unwatch(path).ok();
        }
        let mut errors = Vec::new();
        for path in wanted.watched.difference(&self.targets.watched) {
            if let Err(error) = self.backend.watch(path, RecursiveMode::NonRecursive) {
                errors.push(WatchError::Backend(Box::new(error)));
            }
        }
        self.targets = wanted;
        errors
    }

    fn reload(&mut self) {
        let resolved = self.section.resolve(&Snapshot::read(&self.sources));
        if resolved.diagnostics != self.diagnostics {
            self.diagnostics.clone_from(&resolved.diagnostics);
            (self.subscriber)(Event::Diagnostics(resolved.diagnostics));
        }
        if resolved.settings != self.settings {
            self.settings = resolved.settings.clone();
            (self.subscriber)(Event::Changed(resolved.settings));
        }
    }
}

/// The directories watched, and the files they are watched for.
#[derive(Debug, Default, PartialEq, Eq)]
struct Targets {
    watched: BTreeSet<PathBuf>,
    /// Every layer's file, and where it points when it is a link.
    files: Vec<PathBuf>,
}

impl Targets {
    fn for_files<'a>(files: impl Iterator<Item = &'a Path>) -> Self {
        let files: Vec<PathBuf> = files
            .flat_map(|file| {
                let target = file.canonicalize().ok().filter(|target| target != file);
                std::iter::once(file.to_path_buf()).chain(target)
            })
            .collect();
        let watched = files
            .iter()
            .filter_map(|file| file.ancestors().skip(1).find(|dir| dir.is_dir()))
            .map(Path::to_path_buf)
            .collect();
        Self { watched, files }
    }

    /// Whether a change at `path` may affect a file: it is the file, or a
    /// directory on the way to it.
    fn concerns(&self, path: &Path) -> bool {
        self.files.iter().any(|file| file.starts_with(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watches_the_nearest_existing_directory_of_each_file() {
        let root = tempfile::tempdir().unwrap();
        let present = root.path().join("present");
        std::fs::create_dir(&present).unwrap();
        let files = [
            present.join("config.toml"),
            root.path().join("absent/deskrinch/config.toml"),
        ];
        let targets = Targets::for_files(files.iter().map(PathBuf::as_path));
        let expected: BTreeSet<PathBuf> = [present.clone(), root.path().to_path_buf()].into();
        assert_eq!(targets.watched, expected);
        assert!(targets.concerns(&files[0]));
        assert!(targets.concerns(&root.path().join("absent")));
        assert!(!targets.concerns(&present.join("other.toml")));
        assert!(!targets.concerns(&present.join(".config.toml.swp")));
    }
}

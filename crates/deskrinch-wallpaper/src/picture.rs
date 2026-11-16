//! The picture on screen: its file followed, and the worker's images
//! registered with rinch.
//!
//! The wallpaper decodes the picture itself ([`crate::worker`]) and hands
//! rinch pixels already made for each output, through rinch's registry of
//! RGBA images (`register_rgba`, whose sources are `rinch-image:<name>`):
//! rinch keeps the same buffer the worker made, with no copy of its own.
//!
//! The file is watched ([`Follower`]): a picture replaced, edited, made or
//! taken away after the wallpaper started is decoded again.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use deskrinch_watch::{Event, Target, Timing, Watcher};
use rinch::image::{register_rgba, rgba_src, unregister_rgba};

use crate::worker::Shown;

/// Follows the picture's file: whenever it changes, `changed` is called.
#[derive(Debug, Default)]
pub(crate) struct Follower {
    /// The file followed, and its watcher.
    following: Option<(PathBuf, Watcher)>,
}

impl Follower {
    /// Follows `path`, or nothing; the file followed before is let go.
    pub(crate) fn follow(&mut self, path: Option<&Path>, changed: impl Fn() + Send + 'static) {
        if self
            .following
            .as_ref()
            .map(|(followed, _)| followed.as_path())
            == path
        {
            return;
        }
        self.following = None;
        let Some(path) = path else {
            return;
        };
        let started = Watcher::start(
            vec![Target::File(path.to_owned())],
            Timing::default(),
            "wallpaper-watch",
            move |event| match event {
                Event::Changed => changed(),
                Event::Error(error) => {
                    tracing::warn!("wallpaper: cannot follow its picture any more: {error}");
                }
            },
        );
        match started {
            Ok(watcher) => self.following = Some((path.to_owned(), watcher)),
            Err(error) => tracing::warn!(
                "wallpaper: cannot follow {}: {error}; a change to it shows at the next start",
                path.display()
            ),
        }
    }
}

/// The images registered with rinch, kept in step with what the worker
/// hands the UI: on the UI thread, in one turn, the new images are
/// registered first, the outputs' styles then point at them (the caller's
/// effects, which run after this one), and the images no output shows any
/// more are let go last, so no output is ever pointed at a name that is not
/// there.
#[derive(Debug, Default)]
pub(crate) struct Registered {
    names: BTreeSet<String>,
}

impl Registered {
    /// Registers what `shown` has that is not registered yet, and lets go
    /// of what it no longer has.
    pub(crate) fn follow(&mut self, shown: &Shown) {
        for (name, image) in &shown.pictures {
            if self.names.contains(name) {
                continue;
            }
            match register_rgba(name, image.width(), image.height(), image.shared()) {
                Ok(_) => {
                    self.names.insert(name.clone());
                }
                Err(error) => tracing::warn!("wallpaper: rinch refused an image: {error}"),
            }
        }
        let gone: Vec<String> = self
            .names
            .iter()
            .filter(|name| !shown.pictures.contains_key(*name))
            .cloned()
            .collect();
        for name in gone {
            unregister_rgba(&name);
            self.names.remove(&name);
        }
    }
}

/// The image source of the registered image `name`.
pub(crate) fn source(name: &str) -> String {
    rgba_src(name)
}

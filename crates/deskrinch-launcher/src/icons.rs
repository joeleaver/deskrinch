//! The icon theme's index, followed for the UI: built on a thread of its
//! own for the theme the UI resolved (`deskrinch_theme::resolve_icon_theme`),
//! again when that theme changes, and again when its files do
//! (`deskrinch-icons`' `Watcher`).

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use deskrinch_component::{Handover, report};
use deskrinch_desktop_entries::Icon;
use deskrinch_icons::{Event, Index, Request, SearchPath, Watcher};
use rinch::prelude::Signal;

/// The size icons are drawn at, in logical pixels.
pub(crate) const SIZE: u32 = 32;

/// The index of the theme last asked for, once built.
pub(crate) struct Icons {
    themes: Sender<String>,
    index: Arc<Handover<Option<Arc<Index>>>>,
}

impl Icons {
    /// Starts the thread that builds indexes in `search_path`.
    pub(crate) fn start(search_path: SearchPath) -> Self {
        let (themes, asked) = mpsc::channel();
        let index: Arc<Handover<Option<Arc<Index>>>> = Arc::default();
        let deliver = Arc::clone(&index);
        let spawned = std::thread::Builder::new()
            .name("launcher-icons".into())
            .spawn(move || follow(&search_path, &asked, &deliver));
        if let Err(error) = spawned {
            report::error("starting the icon thread", &error);
        }
        Self { themes, index }
    }

    /// What asks for the index of a theme; the newest asked for wins.
    pub(crate) fn follower(&self) -> impl Fn(String) + 'static {
        let themes = self.themes.clone();
        // The thread ends only with the process.
        move |theme| drop(themes.send(theme))
    }

    /// The UI's signal of the index, on the UI thread: `None` until the
    /// first is built.
    pub(crate) fn signal(&self) -> Signal<Option<Arc<Index>>> {
        self.index.signal(None)
    }
}

/// Builds an index for each theme asked for, the newest first, and follows
/// it until another is asked for.
fn follow(
    search_path: &SearchPath,
    asked: &Receiver<String>,
    deliver: &Arc<Handover<Option<Arc<Index>>>>,
) {
    // The watcher of the theme followed now; replacing it ends the last.
    let mut current: Option<Watcher> = None;
    while let Ok(mut theme) = asked.recv() {
        while let Ok(newer) = asked.try_recv() {
            theme = newer;
        }
        let changes = Arc::clone(deliver);
        let started = Watcher::start(search_path.clone(), &theme, move |event| match event {
            Event::Changed(index) => changes.deliver(Some(index)),
            Event::Error(error) => report::error("following the icon theme", &error),
        });
        match started {
            Ok((watcher, index)) => {
                let _previous = current.replace(watcher);
                deliver.deliver(Some(index));
            }
            Err(error) => {
                report::error("following the icon theme", &error);
                let _previous = current.take();
                deliver.deliver(Some(Arc::new(Index::load(search_path, &theme))));
            }
        }
    }
}

/// The file `icon` is drawn from at `scale`, if the index has one.
pub(crate) fn path_of(index: &Index, icon: &Icon, scale: u32) -> Option<String> {
    let name = match icon {
        Icon::Name(name) => name.as_str(),
        Icon::Path(path) => path.to_str()?,
    };
    let found = index.find(&Request::new(name, SIZE).at_scale(scale)).ok()?;
    found.path.to_str().map(str::to_owned)
}

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

/// The icon an application is drawn with when it names none, or one the
/// theme lacks: the naming specification's generic program.
const FALLBACK: &str = "application-x-executable";

/// The file an application with `icon` is drawn from at `scale`: its own
/// icon when the index has it, else [`FALLBACK`]; `None` when the index has
/// neither, and the row keeps an empty square.
pub(crate) fn path_of(index: &Index, icon: Option<&Icon>, scale: u32) -> Option<String> {
    let name = |icon: &Icon| match icon {
        Icon::Name(name) => Some(name.as_str().to_owned()),
        Icon::Path(path) => path.to_str().map(str::to_owned),
    };
    icon.and_then(name)
        .and_then(|name| find(index, &name, scale))
        .or_else(|| find(index, FALLBACK, scale))
}

/// The file `name` is drawn from at `scale`, if the index has one.
fn find(index: &Index, name: &str, scale: u32) -> Option<String> {
    let found = index.find(&Request::new(name, SIZE).at_scale(scale)).ok()?;
    found.path.to_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use deskrinch_icons::{Index, SearchPath};

    use super::*;

    /// A theme `t` under `base` with one 32-pixel directory holding
    /// `icons`, as PNG files.
    fn index(base: &Path, icons: &[&str]) -> Index {
        let theme = base.join("t");
        fs::create_dir_all(theme.join("32")).unwrap();
        fs::write(
            theme.join("index.theme"),
            "[Icon Theme]\nName=T\nComment=A test theme\nDirectories=32\n[32]\nSize=32\nType=Fixed\n",
        )
        .unwrap();
        for icon in icons {
            fs::write(theme.join("32").join(format!("{icon}.png")), "icon").unwrap();
        }
        Index::load(&SearchPath::from_dirs(vec![base.to_path_buf()]), "t")
    }

    fn png(base: &Path, icon: &str) -> String {
        let path: PathBuf = base.join("t/32").join(format!("{icon}.png"));
        path.to_str().unwrap().to_owned()
    }

    #[test]
    fn an_icon_the_theme_has_is_drawn() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(dir.path(), &["files", FALLBACK]);
        let icon = Icon::Name("files".into());
        assert_eq!(
            path_of(&index, Some(&icon), 1),
            Some(png(dir.path(), "files"))
        );
    }

    #[test]
    fn no_icon_or_one_the_theme_lacks_is_drawn_as_a_generic_program() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(dir.path(), &["files", FALLBACK]);
        let missing = Icon::Name("not-in-the-theme".into());
        assert_eq!(path_of(&index, None, 1), Some(png(dir.path(), FALLBACK)));
        assert_eq!(
            path_of(&index, Some(&missing), 1),
            Some(png(dir.path(), FALLBACK))
        );
    }

    #[test]
    fn without_the_generic_icon_either_there_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(dir.path(), &["files"]);
        assert_eq!(path_of(&index, None, 1), None);
    }
}

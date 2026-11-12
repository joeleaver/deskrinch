//! The icon theme's index, followed for the UI ([`Icons`]): built on a
//! thread of its own for the theme the shell uses
//! (`deskrinch_theme::resolve_icon_theme`), again when that theme changes,
//! and again when its files do (`deskrinch-icons`' `Watcher`); and the file
//! an application is drawn from ([`path_of`]).

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use deskrinch_component::{Handover, report};
use deskrinch_desktop_entries::Icon;
use deskrinch_icons::{Event, Index, Request, SearchPath, Watcher};
use deskrinch_theme::{IconThemeChoice, resolve_icon_theme};
use rinch::prelude::{Memo, Signal};
use rinch::reactive::Effect;

/// The index of the theme last asked for, once built.
pub struct Icons {
    themes: Sender<String>,
    search_path: SearchPath,
    index: Arc<Handover<Option<Arc<Index>>>>,
}

impl Icons {
    /// Starts the thread that builds indexes in `search_path`.
    #[must_use]
    pub fn start(search_path: SearchPath) -> Self {
        let (themes, asked) = mpsc::channel();
        let index: Arc<Handover<Option<Arc<Index>>>> = Arc::default();
        let deliver = Arc::clone(&index);
        let thread_path = search_path.clone();
        let spawned = std::thread::Builder::new()
            .name("icons".into())
            .spawn(move || follow(&thread_path, &asked, &deliver));
        if let Err(error) = spawned {
            report::error("starting the icon thread", &error);
        }
        Self {
            themes,
            search_path,
            index,
        }
    }

    /// Follows the icon theme `choice` says, in the UI: resolved against
    /// what is installed whenever its inputs change (a theme not installed
    /// is warned of), and its index built for it.
    pub fn follow_theme(&self, choice: Memo<IconThemeChoice>) {
        let search_path = self.search_path.clone();
        let theme = Memo::new(move || {
            let choice = choice.get();
            let resolved = resolve_icon_theme(&choice.setting, &choice.portal, |name| {
                search_path.has_theme(name)
            });
            for missing in &resolved.not_installed {
                tracing::warn!("the icon theme {missing:?} is not installed");
            }
            resolved.name
        });
        let themes = self.themes.clone();
        // The thread ends only with the process, so a send cannot fail
        // while there is a UI to follow.
        Effect::new(move || drop(themes.send(theme.get())));
    }

    /// The UI's signal of the index, on the UI thread: `None` until the
    /// first is built.
    #[must_use]
    pub fn signal(&self) -> Signal<Option<Arc<Index>>> {
        self.index.signal(None)
    }
}

impl std::fmt::Debug for Icons {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Icons")
            .field("search_path", &self.search_path)
            .finish_non_exhaustive()
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

/// The file an application with `icon` is drawn from, `size` logical pixels
/// square at `scale`: its own icon when the index has it, else the generic
/// program's, `application-x-executable`; `None` when the index has
/// neither, and the component draws none.
#[must_use]
pub fn path_of(index: &Index, icon: Option<&Icon>, size: u32, scale: u32) -> Option<String> {
    let name = |icon: &Icon| match icon {
        Icon::Name(name) => Some(name.as_str().to_owned()),
        Icon::Path(path) => path.to_str().map(str::to_owned),
    };
    icon.and_then(name)
        .and_then(|name| find(index, &name, size, scale))
        .or_else(|| find(index, FALLBACK, size, scale))
}

/// The icon scale to draw at for a window `factor` (rinch's scale factor,
/// fractional or whole): the next whole one up, since a theme's icons come
/// at whole scales.
#[must_use]
pub fn icon_scale(factor: f64) -> u32 {
    match factor {
        f if f > 2.0 => 3,
        f if f > 1.0 => 2,
        _ => 1,
    }
}

/// The file `name` is drawn from at `size` and `scale`, if the index has one.
fn find(index: &Index, name: &str, size: u32, scale: u32) -> Option<String> {
    let found = index.find(&Request::new(name, size).at_scale(scale)).ok()?;
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
    fn the_icon_scale_is_the_next_whole_one_up() {
        assert_eq!(icon_scale(1.0), 1);
        assert_eq!(icon_scale(1.25), 2);
        assert_eq!(icon_scale(2.0), 2);
        assert_eq!(icon_scale(2.5), 3);
    }

    #[test]
    fn an_icon_the_theme_has_is_drawn() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(dir.path(), &["files", FALLBACK]);
        let icon = Icon::Name("files".into());
        assert_eq!(
            path_of(&index, Some(&icon), 32, 1),
            Some(png(dir.path(), "files"))
        );
    }

    #[test]
    fn no_icon_or_one_the_theme_lacks_is_drawn_as_a_generic_program() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(dir.path(), &["files", FALLBACK]);
        let missing = Icon::Name("not-in-the-theme".into());
        assert_eq!(
            path_of(&index, None, 32, 1),
            Some(png(dir.path(), FALLBACK))
        );
        assert_eq!(
            path_of(&index, Some(&missing), 32, 1),
            Some(png(dir.path(), FALLBACK))
        );
    }

    #[test]
    fn without_the_generic_icon_either_there_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let index = index(dir.path(), &["files"]);
        assert_eq!(path_of(&index, None, 32, 1), None);
    }
}

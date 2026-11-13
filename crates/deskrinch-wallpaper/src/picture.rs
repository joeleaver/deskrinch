//! The picture: where it is, how rinch is given it, and watching it.
//!
//! The picture reaches rinch through an image scheme of the wallpaper's
//! own, [`SCHEME`]: the source `wallpaper:/path/to/file` is loaded by
//! [`load`], which reads the file and checks that its header is a picture
//! rinch can decode (PNG, JPEG, GIF or WebP) before handing it over. A
//! picture that cannot be shown is said once, naming the path read, and
//! the wallpaper shows its colour alone; rinch decodes the rest.
//!
//! rinch keeps one decoded copy of a source for the whole process, however
//! many outputs show it, and scales it to each when it paints.
//!
//! The file is watched ([`Follower`]): a picture replaced, edited, or made
//! after the wallpaper started is loaded again, and shown once decoded.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use deskrinch_watch::{Event, Target, Timing, Watcher};
use rinch::image::{ImageLoadResult, reload_image};

/// The wallpaper's image scheme.
pub(crate) const SCHEME: &str = "wallpaper";

/// The image source for the picture at `path`.
pub(crate) fn source(path: &Path) -> String {
    format!("{SCHEME}:{}", path.display())
}

/// Loads the source `src` (`wallpaper:/path`) for rinch: the file's bytes
/// when its header is a picture rinch decodes, otherwise a failure, which
/// is also logged once with the path.
pub(crate) fn load(src: &str) -> ImageLoadResult {
    let path = src
        .strip_prefix(SCHEME)
        .and_then(|rest| rest.strip_prefix(':'));
    let Some(path) = path else {
        return ImageLoadResult::Failed(format!("{src} is not a wallpaper source"));
    };
    match read(Path::new(path)) {
        Ok(bytes) => ImageLoadResult::Loaded(bytes),
        Err(why) => {
            tracing::warn!("wallpaper: cannot show {path}: {why}; showing the colour alone");
            ImageLoadResult::Failed(why)
        }
    }
}

/// The bytes of the picture at `path`, or why it cannot be shown.
fn read(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?
        .into_dimensions()
        .map_err(|error| format!("it is not a picture this can show ({error})"))?;
    Ok(bytes)
}

/// Follows the picture's file: whenever it changes, rinch loads it again.
#[derive(Debug, Default)]
pub(crate) struct Follower {
    /// The file followed, and its watcher.
    following: Option<(PathBuf, Watcher)>,
}

impl Follower {
    /// Follows `path`, or nothing; the file followed before is let go.
    pub(crate) fn follow(&mut self, path: Option<&Path>) {
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
        let src = source(path);
        let started = Watcher::start(
            vec![Target::File(path.to_owned())],
            Timing::default(),
            "wallpaper-watch",
            move |event| match event {
                Event::Changed => reload_image(&src),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×2 PNG.
    fn png() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::new(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn the_source_names_the_scheme_and_the_path() {
        assert_eq!(source(Path::new("/p/a b.png")), "wallpaper:/p/a b.png");
    }

    #[test]
    fn loads_a_picture_and_refuses_what_is_not_one() {
        let dir = tempfile::tempdir().unwrap();
        let picture = dir.path().join("a.png");
        let png = png();
        std::fs::write(&picture, &png).unwrap();
        assert!(matches!(load(&source(&picture)), ImageLoadResult::Loaded(bytes) if bytes == png));

        let text = dir.path().join("notes.png");
        std::fs::write(&text, "not a picture").unwrap();
        let ImageLoadResult::Failed(why) = load(&source(&text)) else {
            panic!("text loaded as a picture");
        };
        assert!(why.contains("not a picture"), "{why}");

        let missing = dir.path().join("missing.png");
        assert!(matches!(
            load(&source(&missing)),
            ImageLoadResult::Failed(_)
        ));
        assert!(matches!(load("other:/x.png"), ImageLoadResult::Failed(_)));
    }
}

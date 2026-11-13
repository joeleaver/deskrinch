//! The picture: where it is, how rinch is given it, and watching it.
//!
//! The picture reaches rinch through an image scheme of the wallpaper's
//! own, [`SCHEME`]: a [`source`] is loaded by [`load`], which reads the
//! file and checks that its header is a picture rinch can decode (PNG,
//! JPEG, GIF or WebP) before handing it over. A picture that cannot be
//! shown is said once, naming the path read, and the wallpaper shows its
//! colour alone; rinch decodes the rest.
//!
//! The source carries the path's bytes in hexadecimal: `wallpaper:2f68…`.
//! rinch hands a `background-image`'s `url()` to its CSS engine, which
//! parses it as a URL and hands the loader that URL's normalised text
//! (`url.as_str()`, in rinch-dom's `computed_style/from_stylo/visual.rs`):
//! a path written as it is would lose what follows a `#` or a `?` to a
//! fragment or a query, and have its spaces, quotes and non-ASCII letters
//! percent-encoded. Hexadecimal digits pass through untouched, and so does
//! every byte of the path, UTF-8 or not.
//!
//! rinch keeps one decoded copy of a source for the whole process, however
//! many outputs show it, and scales it to each when it paints.
//!
//! The file is watched ([`Follower`]): a picture replaced, edited, or made
//! after the wallpaper started is loaded again, and shown once decoded.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Cursor;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use deskrinch_watch::{Event, Target, Timing, Watcher};
use rinch::image::{ImageLoadResult, reload_image};

/// The wallpaper's image scheme.
pub(crate) const SCHEME: &str = "wallpaper";

/// The image source for the picture at `path`: the scheme, and the path's
/// bytes in lower-case hexadecimal.
pub(crate) fn source(path: &Path) -> String {
    let mut source = format!("{SCHEME}:");
    for byte in path.as_os_str().as_bytes() {
        // Writing to a `String` cannot fail.
        let _ = write!(source, "{byte:02x}");
    }
    source
}

/// The path a [`source`] names, or `None` for one that is not a source.
fn path_of(src: &str) -> Option<PathBuf> {
    let digits = src.strip_prefix(SCHEME)?.strip_prefix(':')?;
    if digits.is_empty() || digits.len() % 2 != 0 {
        return None;
    }
    let bytes = (0..digits.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

/// Loads the source `src` for rinch: the file's bytes when its header is a
/// picture rinch decodes, otherwise a failure, which is also logged once
/// with the path.
pub(crate) fn load(src: &str) -> ImageLoadResult {
    let Some(path) = path_of(src) else {
        return ImageLoadResult::Failed(format!("{src} is not a wallpaper source"));
    };
    match read(&path) {
        Ok(bytes) => ImageLoadResult::Loaded(bytes),
        Err(why) => {
            tracing::warn!(
                "wallpaper: cannot show {}: {why}; showing the colour alone",
                path.display()
            );
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
    fn the_source_names_the_scheme_and_the_paths_bytes() {
        assert_eq!(source(Path::new("/a b")), "wallpaper:2f612062");
    }

    #[test]
    fn every_path_comes_back_exactly_from_its_source() {
        let mut odd = b"/home/r\xe9my/".to_vec(); // Latin-1, not UTF-8.
        odd.extend_from_slice("a b#c?d%41\"e\\f(g)h é.png".as_bytes());
        let paths = [
            PathBuf::from(OsString::from_vec(odd)),
            PathBuf::from("/srv/pictures/plain.jpg"),
        ];
        for path in paths {
            let source = source(&path);
            // Only what a URL parser leaves as it is.
            let (scheme, rest) = source.split_once(':').unwrap();
            assert_eq!(scheme, SCHEME);
            assert!(
                rest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{source}"
            );
            assert_eq!(path_of(&source), Some(path));
        }
        for wrong in ["wallpaper:", "wallpaper:abc", "wallpaper:zz", "other:2f", "2f"] {
            assert_eq!(path_of(wrong), None, "{wrong}");
        }
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

//! Finding an icon: the specification's `FindIcon` and `FindBestIcon`, over
//! an [`Index`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::index::{Format, Index, Indexed, Location};

/// The theme every lookup falls back to.
pub const HICOLOR: &str = "hicolor";

/// What to look for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request<'a> {
    /// The icon's name, such as `document-open`, without an extension; or,
    /// for [`Index::find`], an absolute path.
    pub name: &'a str,
    /// The size wanted, in logical pixels.
    pub size: u32,
    /// The scale it is drawn at (2 on a display at 200 %).
    pub scale: u32,
    /// Whether, failing `a-b-c`, to look for `a-b` and then `a`.
    pub fallback: Fallback,
}

/// What to do when the name is not found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fallback {
    /// Nothing: the name or not found.
    #[default]
    None,
    /// Try ever more generic names, dropping the last `-` component each
    /// time (`input-mouse-usb`, then `input-mouse`, then `input`), as the
    /// Icon Naming Specification describes. Each theme is searched for all
    /// of them before its parents, as the Icon Theme Specification's
    /// `FindBestIcon` does: a more specific icon in a parent theme loses to
    /// a more generic one in the theme itself.
    Generic,
}

impl<'a> Request<'a> {
    /// `name` at `size`, scale 1, without fallback.
    #[must_use]
    pub fn new(name: &'a str, size: u32) -> Self {
        Self {
            name,
            size,
            scale: 1,
            fallback: Fallback::None,
        }
    }

    /// The same request at `scale`.
    #[must_use]
    pub fn at_scale(self, scale: u32) -> Self {
        Self { scale, ..self }
    }

    /// The same request with [`Fallback::Generic`].
    #[must_use]
    pub fn with_generic_fallback(self) -> Self {
        Self {
            fallback: Fallback::Generic,
            ..self
        }
    }
}

/// A found icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    /// Its file.
    pub path: PathBuf,
    /// Its format.
    pub format: Format,
}

/// Why a lookup found nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LookupError {
    /// No theme consulted, and no base directory, has the icon.
    #[error("no icon named {name:?}")]
    NotFound {
        /// The name looked for.
        name: String,
    },
    /// The name cannot name an icon: it is empty, or has a `/` without
    /// being an absolute path, or the size or scale is 0.
    #[error("{name:?} at size {size} and scale {scale} is not something to look up")]
    InvalidRequest {
        /// The name.
        name: String,
        /// The size.
        size: u32,
        /// The scale.
        scale: u32,
    },
    /// The name is an absolute path, which only [`Index::find`] answers.
    #[error("{0:?} is a path; Index::find answers paths")]
    Path(PathBuf),
    /// The absolute path is no file.
    #[error("{0:?} is not a file")]
    NoFile(PathBuf),
    /// The absolute path's extension is none of `png`, `svg` and `xpm`.
    #[error("{0:?} is not a PNG, SVG or XPM file")]
    UnsupportedFormat(PathBuf),
}

impl Index {
    /// Looks up `request` in the index's theme, its parents and `hicolor`,
    /// then among the icons outside any theme, by the Icon Theme
    /// Specification. It reads no file.
    ///
    /// Within a theme the first directory (in the theme's order) that
    /// serves the size and scale as they are wins, its first base directory
    /// that has the icon, and the first format of PNG, SVG and XPM. Failing
    /// that, the icon closest in device pixels wins, the first one on a tie.
    /// Only when no theme has the name are the base directories themselves
    /// looked in.
    ///
    /// # Errors
    ///
    /// [`LookupError::NotFound`] when nothing has the icon; an invalid
    /// request, or an absolute path (see [`find`](Self::find)), as such.
    pub fn lookup(&self, request: &Request<'_>) -> Result<Icon, LookupError> {
        if Path::new(request.name).is_absolute() {
            return Err(LookupError::Path(request.name.into()));
        }
        if request.name.is_empty()
            || request.name.contains('/')
            || request.size == 0
            || request.scale == 0
        {
            return Err(LookupError::InvalidRequest {
                name: request.name.into(),
                size: request.size,
                scale: request.scale,
            });
        }
        let names = match request.fallback {
            Fallback::None => vec![request.name],
            Fallback::Generic => generic_names(request.name),
        };
        let mut visited = HashSet::new();
        [self.theme.as_str(), HICOLOR]
            .into_iter()
            .find_map(|theme| self.in_theme(theme, &names, request, &mut visited))
            .or_else(|| names.iter().find_map(|name| self.unthemed(name)))
            .ok_or_else(|| LookupError::NotFound {
                name: request.name.into(),
            })
    }

    /// [`lookup`](Self::lookup), and for a name that is an absolute path,
    /// that file, if it exists and is a PNG, SVG or XPM file. Only an
    /// absolute path makes it read the file system, to see that the file
    /// exists.
    ///
    /// # Errors
    ///
    /// As [`lookup`](Self::lookup); for a path,
    /// [`LookupError::NoFile`] or [`LookupError::UnsupportedFormat`].
    pub fn find(&self, request: &Request<'_>) -> Result<Icon, LookupError> {
        let path = Path::new(request.name);
        if !path.is_absolute() {
            return self.lookup(request);
        }
        let format = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(Format::split)
            .map(|(_, format)| format)
            .ok_or_else(|| LookupError::UnsupportedFormat(path.into()))?;
        if !path.is_file() {
            return Err(LookupError::NoFile(path.into()));
        }
        Ok(Icon {
            path: path.into(),
            format,
        })
    }

    /// `FindBestIconHelper`: `theme`, then its parents depth first, each
    /// theme once.
    fn in_theme<'s>(
        &'s self,
        theme: &'s str,
        names: &[&str],
        request: &Request<'_>,
        visited: &mut HashSet<&'s str>,
    ) -> Option<Icon> {
        if !visited.insert(theme) {
            return None;
        }
        let indexed = self.themes.get(theme)?;
        names
            .iter()
            .find_map(|name| self.in_directories(indexed, name, request))
            .or_else(|| {
                indexed
                    .theme
                    .inherits
                    .iter()
                    .find_map(|parent| self.in_theme(parent, names, request, visited))
            })
    }

    /// `LookupIcon`: the first file whose directory serves the size and
    /// scale, else the closest.
    fn in_directories(&self, theme: &Indexed, name: &str, request: &Request<'_>) -> Option<Icon> {
        let locations = theme.icons.get(name)?;
        let directory = |at: &Location| &theme.theme.directories[at.directory];
        let at = locations
            .iter()
            .find(|at| directory(at).matches(request.size, request.scale))
            .or_else(|| {
                // `min_by_key` keeps the first of equal keys.
                locations
                    .iter()
                    .min_by_key(|at| directory(at).distance(request.size, request.scale))
            })?;
        Some(Icon {
            path: self.themed_path(theme, name, *at),
            format: at.format,
        })
    }

    /// `LookupFallbackIcon`: the first base directory that has the icon.
    fn unthemed(&self, name: &str) -> Option<Icon> {
        let at = *self.unthemed.get(name)?.first()?;
        Some(Icon {
            path: self.unthemed_path(name, at),
            format: at.format,
        })
    }
}

/// `name`, then each name with its last `-` component dropped.
fn generic_names(name: &str) -> Vec<&str> {
    let mut names = vec![name];
    let mut rest = name;
    while let Some(end) = rest.rfind('-') {
        rest = &rest[..end];
        if rest.is_empty() {
            break;
        }
        names.push(rest);
    }
    names
}

#[cfg(test)]
#[path = "lookup_tests.rs"]
mod tests;

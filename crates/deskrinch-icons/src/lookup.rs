//! Finding an icon: the specification's `FindIcon` and `FindBestIcon`, over
//! an [`Index`].

use std::path::{Path, PathBuf};

use crate::candidates::{self, Candidates};
use crate::index::{Format, Index};

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
    ///
    /// A symbolic name prefers symbolic icons at every level before any
    /// coloured one, since a monochrome icon was asked for:
    /// `input-mouse-symbolic`, then `input-symbolic`, then `input-mouse`,
    /// then `input`.
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
}

impl Index {
    /// Every file that could answer `request`, best first (see
    /// [`Candidates`] for the order), without reading any: the index lists
    /// names, and a name may be a broken link. [`find`](Self::find) takes
    /// the first that exists; [`lookup`](Self::lookup) the first.
    ///
    /// # Errors
    ///
    /// An invalid request, or an absolute path (see [`find`](Self::find)),
    /// as such.
    pub fn candidates(&self, request: &Request<'_>) -> Result<Candidates<'_>, LookupError> {
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
        let generic = request.fallback == Fallback::Generic;
        let names = candidates::names(request.name, generic);
        Ok(Candidates::new(self, names, request.size, request.scale))
    }

    /// Looks up `request` in the index's theme, its parents and `hicolor`,
    /// then among the icons outside any theme, by the Icon Theme
    /// Specification: the first of the [`candidates`](Self::candidates).
    /// It reads no file, so the file may turn out to be a broken link;
    /// [`find`](Self::find) checks.
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
    /// request, or an absolute path, as such.
    pub fn lookup(&self, request: &Request<'_>) -> Result<Icon, LookupError> {
        self.candidates(request)?
            .next()
            .ok_or_else(|| LookupError::NotFound {
                name: request.name.into(),
            })
    }

    /// The first of the [`candidates`](Self::candidates) that exists, so a
    /// broken link is never returned; and for a name that is an absolute
    /// path, that file, if it exists, in whatever format. It reads the file
    /// system once per candidate it checks: once, unless it meets a broken
    /// link.
    ///
    /// # Errors
    ///
    /// As [`lookup`](Self::lookup); for a path, [`LookupError::NoFile`].
    pub fn find(&self, request: &Request<'_>) -> Result<Icon, LookupError> {
        let path = Path::new(request.name);
        if path.is_absolute() {
            if !path.is_file() {
                return Err(LookupError::NoFile(path.into()));
            }
            return Ok(Icon {
                path: path.into(),
                format: candidates::format_of(path),
            });
        }
        self.candidates(request)?
            .find(|icon| icon.path.is_file())
            .ok_or_else(|| LookupError::NotFound {
                name: request.name.into(),
            })
    }
}

#[cfg(test)]
#[path = "lookup_tests.rs"]
mod tests;

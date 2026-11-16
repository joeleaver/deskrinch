//! The worker's state: what is wanted, the picture, and what was handed to
//! the UI, stepped by what came in. No thread and no rinch here: the
//! pictures are made through the decoder it is given, and what the UI is to
//! show is the value [`State::step`] returns.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use deskrinch_picture::{DecodeError, Fit as Fitting, FitError, crop, fit};
use deskrinch_settings::wallpaper::Fit;
use deskrinch_untrusted::Image;
use deskrinch_untrusted::file::FileError;
use rinch::OutputId;

use crate::limits::{LIMITS, why};
use crate::plan::{Need, Surface, plan};

/// What the UI asks for: the picture's path (none: the colour alone), how
/// it is fitted, and every output's surface that has a size yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Wanted {
    /// The picture's path, `~/` expanded.
    pub(crate) path: Option<PathBuf>,
    /// The fit the settings name.
    pub(crate) fit: Fit,
    /// Each output's surface.
    pub(crate) surfaces: BTreeMap<OutputId, Surface>,
}

/// What came in since the last step.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Input {
    /// What the UI wants now.
    Wanted(Wanted),
    /// The picture's file changed (written, replaced, made or taken away).
    Changed,
}

/// What the UI is to show: every image that is shown, by its name in
/// rinch's registry, and each output's name. An output with no name shows
/// the colour alone.
///
/// Complete each time: the UI registers the names it does not have yet,
/// points each output at its name, and only then lets go of the names no
/// output shows any more.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Shown {
    /// The images, by name; the same buffers rinch is given, shared.
    pub(crate) pictures: BTreeMap<String, Image>,
    /// Each output's image, by name.
    pub(crate) names: BTreeMap<OutputId, String>,
}

/// The worker's state, decoding through `D`.
pub(crate) struct State<D> {
    /// Decodes the picture at a path.
    decode: D,
    /// What was wanted at the last step.
    wanted: Wanted,
    /// Counts the pictures decoded from a changed file: part of every name,
    /// so a name always means the same pixels.
    generation: u64,
    /// The decoded picture: held while pixels are made from it, and kept
    /// only while it is itself shown (when rinch holds the same buffer, so
    /// keeping it costs nothing).
    picture: Option<Image>,
    /// The decoded picture's size, kept after it is let go: what the plan
    /// is made from.
    size: Option<(u32, u32)>,
    /// What the UI was last handed.
    shown: Shown,
}

impl<D> std::fmt::Debug for State<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("wanted", &self.wanted)
            .field("generation", &self.generation)
            .field("holds_picture", &self.picture.is_some())
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl<D: FnMut(&Path) -> Result<Image, DecodeError>> State<D> {
    /// A state that wants nothing yet, decoding with `decode`.
    pub(crate) fn new(decode: D) -> Self {
        Self {
            decode,
            wanted: Wanted::default(),
            generation: 0,
            picture: None,
            size: None,
            shown: Shown::default(),
        }
    }

    /// What `inputs`, everything that came in since the last step, make the
    /// UI show, if that changed.
    pub(crate) fn step(&mut self, inputs: impl IntoIterator<Item = Input>) -> Option<Shown> {
        let mut changed = false;
        let mut wanted = None;
        for input in inputs {
            match input {
                Input::Wanted(new) => wanted = Some(new),
                Input::Changed => changed = true,
            }
        }
        if let Some(wanted) = wanted {
            if wanted.path != self.wanted.path {
                // Another picture: nothing of the last one is kept.
                self.picture = None;
                self.size = None;
            }
            self.wanted = wanted;
        }
        let next = self.next(changed);
        (next != self.shown).then(|| {
            self.shown = next.clone();
            next
        })
    }

    /// What is shown next, the file having `changed` or not.
    fn next(&mut self, changed: bool) -> Shown {
        let Some(path) = self.wanted.path.clone() else {
            self.picture = None;
            self.size = None;
            return Shown::default();
        };
        if changed || self.size.is_none() {
            match self.read(&path) {
                Read::Fresh => {}
                Read::Keep => return self.shown.clone(),
                Read::Colour => return Shown::default(),
            }
        }
        let next = self.made(&path);
        // Kept only while it is itself shown, when rinch holds it anyway.
        if !next
            .pictures
            .contains_key(&Need::Picture.name(self.generation))
        {
            self.picture = None;
        }
        next
    }

    /// Decodes the picture at `path` as it is now, with a new generation. A
    /// picture that cannot be shown is said once here; one that was shown
    /// and is replaced by one that cannot be keeps what was shown, and a
    /// file taken away shows the colour.
    fn read(&mut self, path: &Path) -> Read {
        match (self.decode)(path) {
            Ok(image) => {
                self.generation += 1;
                self.size = Some((image.width(), image.height()));
                self.picture = Some(image);
                Read::Fresh
            }
            Err(error) if gone(&error) && self.size.is_some() => {
                tracing::warn!(
                    "wallpaper: {} is gone; showing the colour alone",
                    path.display()
                );
                self.forget();
                Read::Colour
            }
            Err(error) if self.size.is_some() => {
                tracing::warn!(
                    "wallpaper: cannot show {} as it is now: {}; keeping the picture shown before",
                    path.display(),
                    why(&error)
                );
                Read::Keep
            }
            Err(error) => {
                tracing::warn!(
                    "wallpaper: cannot show {}: {}; showing the colour alone",
                    path.display(),
                    why(&error)
                );
                self.forget();
                Read::Colour
            }
        }
    }

    /// Nothing of the picture is kept.
    fn forget(&mut self) {
        self.picture = None;
        self.size = None;
    }

    /// Every output's image, made from the picture where the UI does not
    /// have it yet.
    fn made(&mut self, path: &Path) -> Shown {
        let Some(size) = self.size else {
            return Shown::default();
        };
        let needs = plan(size, self.wanted.fit, &self.wanted.surfaces);
        let mut next = Shown::default();
        for (&output, &need) in &needs {
            let name = need.name(self.generation);
            if !next.pictures.contains_key(&name) {
                let image = match self.shown.pictures.get(&name) {
                    Some(image) => Some(image.clone()),
                    None => self.make(path, need),
                };
                let Some(image) = image else {
                    continue;
                };
                next.pictures.insert(name.clone(), image);
            }
            next.names.insert(output, name);
        }
        next
    }

    /// The image `need` asks for, from the picture, decoded again if it was
    /// let go; `None` (said) when it cannot be made.
    fn make(&mut self, path: &Path, need: Need) -> Option<Image> {
        if self.picture.is_none() {
            match (self.decode)(path) {
                // The same file as the generation's, unless it changed and
                // its change is still on its way, which brings the next one.
                Ok(image) => self.picture = Some(image),
                Err(error) => {
                    tracing::warn!(
                        "wallpaper: cannot show {} on a new output: {}",
                        path.display(),
                        why(&error)
                    );
                    return None;
                }
            }
        }
        let picture = self.picture.as_ref()?;
        let made = match need {
            Need::Picture => Ok(picture.clone()),
            Need::Cover { width, height } => fit(picture, width, height, Fitting::Cover, &LIMITS),
            Need::Scaled { width, height } => {
                fit(picture, width, height, Fitting::Stretch, &LIMITS)
            }
            Need::Crop {
                x,
                y,
                width,
                height,
            } => crop(picture, x, y, width, height, &LIMITS),
        };
        made.map_err(|error: FitError| {
            tracing::warn!(
                "wallpaper: cannot fit {} to an output: {error}",
                path.display()
            );
        })
        .ok()
    }

    /// Whether the picture is held now: for the tests.
    #[cfg(test)]
    pub(crate) fn holds_picture(&self) -> bool {
        self.picture.is_some()
    }
}

/// What reading the picture again came to.
enum Read {
    /// A picture, of a new generation.
    Fresh,
    /// None; what was shown stays.
    Keep,
    /// None; the colour alone.
    Colour,
}

/// Whether `error` says the file is not there.
fn gone(error: &DecodeError) -> bool {
    matches!(error, DecodeError::File(FileError::Io(io)) if io.kind() == ErrorKind::NotFound)
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;

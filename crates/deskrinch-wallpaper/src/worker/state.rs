//! The worker's state: what is wanted, the picture, and what was handed to
//! the UI, stepped by what came in. No thread, no rinch and no
//! `deskrinch-picture` here: the picture is decoded, and each output's
//! image made from it, through the functions it is given, and what the UI
//! is to show is the value [`State::step`] returns.

use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use deskrinch_picture::{DecodeError, FitError};
use deskrinch_settings::wallpaper::Fit;
use deskrinch_untrusted::Image;
use deskrinch_untrusted::file::FileError;
use rinch::OutputId;

use crate::limits::why;
use crate::plan::{Need, Surface, plan};

/// What the UI asks for: the picture's path (none: the colour alone), how
/// it is fitted, every output listed, and every output's surface that has
/// a size yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Wanted {
    /// The picture's path, `~/` expanded.
    pub(crate) path: Option<PathBuf>,
    /// The fit the settings name.
    pub(crate) fit: Fit,
    /// Every output listed (rinch's `output_ids()`), with a size or not:
    /// the outputs the picture is decoded for, before their windows are
    /// configured.
    pub(crate) outputs: BTreeSet<OutputId>,
    /// Each output's surface, once it has a size, while its window's row
    /// lasts.
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

/// What decodes the picture at a path: the worker's decoder, or a test's.
pub(crate) trait Decode: FnMut(&Path) -> Result<Image, DecodeError> {}

impl<T: FnMut(&Path) -> Result<Image, DecodeError>> Decode for T {}

/// What makes the image a [`Need`] asks for from the picture: the worker's
/// maker, or a test's.
pub(crate) trait Make: FnMut(&Image, Need) -> Result<Image, FitError> {}

impl<T: FnMut(&Image, Need) -> Result<Image, FitError>> Make for T {}

/// The worker's state, decoding through `D` and making each output's image
/// from the picture through `M`.
pub(crate) struct State<D, M> {
    /// Decodes the picture at a path.
    decode: D,
    /// Makes the image a [`Need`] asks for from the picture.
    make: M,
    /// What was wanted at the last step.
    wanted: Wanted,
    /// Counts the pictures decoded from a changed file: part of every name,
    /// so a name always means the same pixels.
    generation: u64,
    /// The decoded picture: held while any listed output still lacks its
    /// pixels (from the moment an output is listed, before any has a size,
    /// so that the decode runs while the windows open), and then only while
    /// it is itself shown (when rinch holds the same buffer, so keeping it
    /// costs nothing).
    picture: Option<Image>,
    /// The decoded picture's size, kept after it is let go: what the plan
    /// is made from.
    size: Option<(u32, u32)>,
    /// The path's file, as it is now, is known not to decode: its last
    /// decode failed (not a picture, over the limits, missing, or taken
    /// away), at a read or when the picture let go was decoded again for a
    /// new image, whether the colour or a kept picture is shown since.
    /// Nothing decodes it again until it changes, as the file's watcher
    /// says ([`Input::Changed`]: written, made, or taken away), or the path
    /// changes: never on a timer, and never because an output came, went
    /// or got a size, or the fit changed. So a bad file is decoded and said
    /// once.
    unreadable: bool,
    /// Each listed output's last surface: what the plan is made over. Kept
    /// while the output is listed, whether its window's row is there or
    /// not (the rows go one by one when the wallpaper ends, while every
    /// output is still listed, and nothing is planned again for that);
    /// replaced when a surface has a new size; dropped when the output is
    /// no longer listed and has no surface. An output listed with no entry
    /// is one still to be served.
    last: BTreeMap<OutputId, Surface>,
    /// The needs whose image was refused under this generation (said once
    /// each): not made again until the picture, the path or the need
    /// changes; an output that asks for one keeps the image it shows.
    refused: BTreeSet<Need>,
    /// What the UI was last handed.
    shown: Shown,
}

impl<D, M> std::fmt::Debug for State<D, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("wanted", &self.wanted)
            .field("generation", &self.generation)
            .field("holds_picture", &self.picture.is_some())
            .field("size", &self.size)
            .field("unreadable", &self.unreadable)
            .field("last", &self.last)
            .field("refused", &self.refused)
            .finish_non_exhaustive()
    }
}

impl<D: Decode, M: Make> State<D, M> {
    /// A state that wants nothing yet, decoding with `decode` and making
    /// each output's image with `make`.
    pub(crate) fn new(decode: D, make: M) -> Self {
        Self {
            decode,
            make,
            wanted: Wanted::default(),
            generation: 0,
            picture: None,
            size: None,
            unreadable: false,
            last: BTreeMap::new(),
            refused: BTreeSet::new(),
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
                // Another picture: nothing of the last one is kept, not
                // even its failure or its refusals.
                self.forget();
                self.unreadable = false;
                self.refused.clear();
            }
            self.wanted = wanted;
            self.last.extend(&self.wanted.surfaces);
            let (outputs, surfaces) = (&self.wanted.outputs, &self.wanted.surfaces);
            self.last
                .retain(|output, _| outputs.contains(output) || surfaces.contains_key(output));
        }
        let next = self.next(changed);
        (next != self.shown).then(|| {
            self.shown = next.clone();
            next
        })
    }

    /// What is shown next, the file having `changed` or not.
    fn next(&mut self, changed: bool) -> Shown {
        if changed {
            // Something new to read, whatever is listed now.
            self.unreadable = false;
        }
        let Some(path) = self.wanted.path.clone() else {
            self.forget();
            self.unreadable = false;
            return Shown::default();
        };
        if !self.listed() {
            // No output is connected: a wallpaper with nowhere to show
            // decodes nothing. What changes meanwhile is read afresh when
            // one is.
            self.forget();
            return Shown::default();
        }
        if (self.size.is_none() || changed) && !self.unreadable {
            // At start this runs as soon as an output is listed, before
            // any window has a size: the decode needs none, so it runs
            // while the windows open, and only the fit waits for a size.
            // Whatever it comes to, the plan is made next: over no picture
            // (the colour), or over the one kept, where each output keeps
            // what it shows and nothing is decoded while the file is bad.
            self.read(&path);
        }
        let next = self.made(&path);
        if self.pending() {
            // An output still to be served: the picture is held for it, and
            // decoded again for one listed after it was let go.
            self.hold(&path);
        } else if !next
            .pictures
            .contains_key(&Need::Picture.name(self.generation))
        {
            // Every output served: kept only while it is itself shown, when
            // rinch holds it anyway.
            self.picture = None;
        }
        next
    }

    /// Whether any output is listed (a surface counts as listing its
    /// output, whatever order the UI's two lists arrive in).
    fn listed(&self) -> bool {
        !self.wanted.outputs.is_empty() || !self.last.is_empty()
    }

    /// Whether a listed output still lacks its pixels: it has had no
    /// surface since it was listed.
    fn pending(&self) -> bool {
        self.wanted
            .outputs
            .iter()
            .any(|output| !self.last.contains_key(output))
    }

    /// Decodes the picture at `path` as it is now, with a new generation. A
    /// picture that cannot be shown is said once here; one that was shown
    /// and is replaced by one that cannot be keeps what was shown, and a
    /// file taken away shows the colour.
    fn read(&mut self, path: &Path) {
        match (self.decode)(path) {
            Ok(image) => {
                self.generation += 1;
                self.refused.clear();
                self.size = Some((image.width(), image.height()));
                self.picture = Some(image);
            }
            Err(error) if gone(&error) && self.size.is_some() => {
                tracing::warn!(
                    "wallpaper: {} is gone; showing the colour alone",
                    path.display()
                );
                self.forget();
                self.unreadable = true;
            }
            Err(error) if self.size.is_some() => {
                tracing::warn!(
                    "wallpaper: cannot show {} as it is now: {}; keeping the picture shown before",
                    path.display(),
                    why(&error)
                );
                self.unreadable = true;
            }
            Err(error) => {
                tracing::warn!(
                    "wallpaper: cannot show {}: {}; showing the colour alone",
                    path.display(),
                    why(&error)
                );
                self.forget();
                self.unreadable = true;
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
        let needs = plan(size, self.wanted.fit, &self.last);
        let mut next = Shown::default();
        for (&output, &need) in &needs {
            let name = need.name(self.generation);
            if !next.pictures.contains_key(&name) {
                let image = match self.shown.pictures.get(&name) {
                    Some(image) => Some(image.clone()),
                    None => self.make(path, need),
                };
                let Some(image) = image else {
                    self.keep_shown(output, &mut next);
                    continue;
                };
                next.pictures.insert(name.clone(), image);
            }
            next.names.insert(output, name);
        }
        next
    }

    /// `output` keeps the image it shows, if it shows one, when its new one
    /// cannot be made (the file gone bad while its picture is kept, or the
    /// image refused): drawn by the new fit's CSS until the file is good
    /// again, as old pixels are while new ones are made. An output new to
    /// the picture has none, and shows the colour.
    fn keep_shown(&self, output: OutputId, next: &mut Shown) {
        if let Some(name) = self.shown.names.get(&output) {
            next.pictures
                .insert(name.clone(), self.shown.pictures[name].clone());
            next.names.insert(output, name.clone());
        }
    }

    /// The image `need` asks for, from the picture, decoded again if it was
    /// let go; `None` when it cannot be made: said once, at the failed
    /// decode or at its refusal, and not tried again while that holds.
    fn make(&mut self, path: &Path, need: Need) -> Option<Image> {
        if self.refused.contains(&need) || !self.hold(path) {
            return None;
        }
        let picture = self.picture.as_ref()?;
        (self.make)(picture, need)
            .map_err(|error: FitError| {
                tracing::warn!(
                    "wallpaper: cannot fit {} to an output: {error}",
                    path.display()
                );
                self.refused.insert(need);
            })
            .ok()
    }

    /// Whether the picture is held, decoded again if it was let go: not
    /// when it cannot be (said at the failed decode), and quietly, with no
    /// decode, while the file is known not to decode.
    fn hold(&mut self, path: &Path) -> bool {
        if self.picture.is_some() {
            return true;
        }
        if self.unreadable {
            return false;
        }
        match (self.decode)(path) {
            // The same file as the generation's, unless it changed and its
            // change is still on its way, which brings the next one.
            Ok(image) => {
                self.picture = Some(image);
                true
            }
            Err(error) => {
                tracing::warn!(
                    "wallpaper: cannot show {} on a new output: {}",
                    path.display(),
                    why(&error)
                );
                self.unreadable = true;
                false
            }
        }
    }

    /// Whether the picture is held now: for the tests.
    #[cfg(test)]
    pub(crate) fn holds_picture(&self) -> bool {
        self.picture.is_some()
    }
}

/// Whether `error` says the file is not there.
fn gone(error: &DecodeError) -> bool {
    matches!(error, DecodeError::File(FileError::Io(io)) if io.kind() == ErrorKind::NotFound)
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;

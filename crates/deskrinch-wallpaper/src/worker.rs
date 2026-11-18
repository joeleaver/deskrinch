//! The picture's worker: a thread that decodes the picture and makes each
//! output's pixels, off the UI thread, and hands the UI what to show.
//!
//! The UI tells it what it wants (the path, the fit, every output listed
//! and every output's surface: [`Worker::want`]), and the file's watcher
//! that the file changed ([`Worker::changed`]). Everything that came in
//! while it worked is one step ([`State::step`]): a burst of outputs
//! configured at once, or a file written in several closes, is one decode,
//! and a size that came while the picture was decoded is fitted after it
//! with no second decode. What the UI is to show goes back through a
//! hand-over ([`Shown`]), complete each time, so the newest is all the UI
//! needs.
//!
//! The picture is decoded as soon as an output is listed, while its window
//! opens, and held while any listed output still lacks its pixels; then
//! only while it is itself shown, when rinch holds the same buffer. The
//! next change that needs pixels decodes it again ([`State`]). The thread
//! ends when the last sender (the UI's and the watcher's) is dropped.

use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};

use deskrinch_component::Handover;
use deskrinch_picture::{DecodeError, Fit as Fitting, FitError, Profile, crop, decode_file, fit};
use deskrinch_untrusted::Image;

use crate::limits::{FORMATS, LIMITS};
use crate::plan::Need;

mod state;

pub(crate) use state::{Input, Shown, State, Wanted};

/// The worker, from the UI's side: where its inputs go.
#[derive(Debug, Clone)]
pub(crate) struct Worker {
    input: Sender<Input>,
}

impl Worker {
    /// Starts the worker's thread, which hands what is to be shown to
    /// `deliver`.
    ///
    /// # Errors
    ///
    /// The thread could not be started.
    pub(crate) fn start(deliver: Handover<Shown>) -> io::Result<Self> {
        let (input, inputs) = mpsc::channel();
        std::thread::Builder::new()
            .name("wallpaper-picture".to_owned())
            .spawn(move || run(&inputs, decode, |shown| deliver.deliver(shown)))?;
        Ok(Self { input })
    }

    /// What the UI wants now.
    pub(crate) fn want(&self, wanted: Wanted) {
        tracing::debug!("wallpaper: wanted {wanted:?}");
        // The worker ends only when every sender is gone, and this one is
        // not: the send cannot fail.
        self.input.send(Input::Wanted(wanted)).ok();
    }

    /// Where the file's watcher says the picture's file changed.
    pub(crate) fn changed(&self) -> impl Fn() + Send + 'static {
        let input = self.input.clone();
        move || {
            // As in `want`: the worker outlives this sender.
            input.send(Input::Changed).ok();
        }
    }
}

/// The worker's loop: everything that came in while it worked is one step,
/// decoding through `decode` and making each output's image with
/// [`make`], and what is to be shown goes to `deliver`. Ends when every
/// sender of `inputs` is gone.
fn run(
    inputs: &Receiver<Input>,
    decode: impl FnMut(&Path) -> Result<Image, DecodeError>,
    mut deliver: impl FnMut(Shown),
) {
    let mut state = State::new(decode, make);
    while let Ok(first) = inputs.recv() {
        let more = inputs.try_iter();
        if let Some(shown) = state.step(std::iter::once(first).chain(more)) {
            tracing::debug!(
                "wallpaper: {} images for {} outputs: {:?}",
                shown.pictures.len(),
                shown.names.len(),
                shown.names
            );
            deliver(shown);
        }
    }
}

/// The picture at `path`, decoded under the wallpaper's limits: sRGB,
/// upright. A colour profile that could not be used is said, at debug.
fn decode(path: &Path) -> Result<Image, DecodeError> {
    let decoded = decode_file(path, FORMATS, &LIMITS)?;
    if let Profile::Ignored(why) = &decoded.profile {
        tracing::debug!(
            "wallpaper: the colour profile of {} is not used: {why:?}",
            path.display()
        );
    }
    Ok(decoded.image)
}

/// The image `need` asks for, made from `picture` under the wallpaper's
/// limits.
fn make(picture: &Image, need: Need) -> Result<Image, FitError> {
    match need {
        Need::Picture => Ok(picture.clone()),
        Need::Cover { width, height } => fit(picture, width, height, Fitting::Cover, &LIMITS),
        Need::Scaled { width, height } => fit(picture, width, height, Fitting::Stretch, &LIMITS),
        Need::Crop {
            x,
            y,
            width,
            height,
        } => crop(picture, x, y, width, height, &LIMITS),
    }
}

#[cfg(test)]
#[path = "worker/run_tests.rs"]
mod tests;

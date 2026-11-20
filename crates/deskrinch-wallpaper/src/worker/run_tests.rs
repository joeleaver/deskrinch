//! The worker's loop, with a decoder the test holds: a size that comes
//! while the picture is decoded is fitted after it, with no second decode.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deskrinch_settings::wallpaper::Fit;
use rinch::OutputId;

use super::*;
use crate::plan::Surface;

/// How long the test waits for the worker before calling it stuck.
const PATIENCE: Duration = Duration::from_secs(10);

fn wanted(surfaces: &[(u64, (u32, u32))]) -> Input {
    Input::Wanted(Wanted {
        path: Some(PathBuf::from("/pictures/a.png")),
        fit: Fit::Cover,
        outputs: BTreeSet::from([OutputId::from_raw(1)]),
        surfaces: surfaces
            .iter()
            .map(|&(id, size)| {
                (
                    OutputId::from_raw(id),
                    Surface {
                        device: size,
                        logical: size,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
    })
}

#[test]
fn a_size_that_comes_while_the_picture_is_decoded_is_one_decode_then_the_fit() {
    let (input, inputs) = mpsc::channel();
    let (started, decoding) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let released = Mutex::new(released);
    let decodes = Arc::new(AtomicU32::new(0));
    let counted = Arc::clone(&decodes);
    let (deliver, delivered) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        run(
            &inputs,
            move |_: &Path| {
                counted.fetch_add(1, Ordering::SeqCst);
                started.send(()).unwrap();
                // Held until the test has sent the size.
                released.lock().unwrap().recv_timeout(PATIENCE).unwrap();
                Ok(Image::filled(3840, 2160, |rgba| rgba.fill(u8::MAX)).unwrap())
            },
            move |shown| deliver.send(shown).unwrap(),
        );
    });
    // The output is listed, with no size yet: the decode starts.
    input.send(wanted(&[])).unwrap();
    decoding
        .recv_timeout(PATIENCE)
        .expect("the decode starts before any output has a size");
    // Its size comes while the decode runs.
    input.send(wanted(&[(1, (1280, 720))])).unwrap();
    release.send(()).unwrap();
    let shown = delivered.recv_timeout(PATIENCE).unwrap();
    assert_eq!(
        shown.names[&OutputId::from_raw(1)],
        "wallpaper/1/cover/1280x720"
    );
    let image = &shown.pictures["wallpaper/1/cover/1280x720"];
    assert_eq!((image.width(), image.height()), (1280, 720));
    assert_eq!(decodes.load(Ordering::SeqCst), 1);
    drop(input);
    worker.join().unwrap();
}

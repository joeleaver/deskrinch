//! The wallpaper's root: its settings as a signal, the picture's worker,
//! and a window on every output.
//!
//! The root is the document of an app with no window of its own
//! (`App::windowless`): it draws nothing, and holds one `Window` per
//! output, a row of a `for` over rinch's `output_ids()`, keyed by the id.
//! A row's window opens with its row and closes with it, when its output
//! is disconnected or turned off, or when its output goes first
//! ([`surface::closed`]).
//!
//! The row's item is the output's id alone, not rinch's whole `Output`: a
//! `for` row is drawn again, its window closed and a new one opened, when
//! its item changes, and an `Output` changes with the output's position,
//! size and scale. An id does not change while the output is connected, so
//! an output that is moved, rescaled, turned or given another mode keeps
//! its row and its window, whose layer surface the compositor reconfigures.
//! `output_ids()` is the same list while the same outputs are connected, so
//! such a change does not even run the `for` again; only an output coming
//! or going does. Before any of it, the root asks whether the display
//! offers a layer shell at all ([`role`]), and opens nothing if it does
//! not.
//!
//! Each window tells the root its surface's size, in device pixels
//! (rinch's `window_physical_size`, the buffer's own) and logical pixels
//! (its `window_size`), as rinch reports them for that window; the root
//! tells the worker the path, the fit, every output listed and every
//! surface ([`crate::worker`]): the outputs listed let the decode start
//! before any window has a size, and the surfaces let each fit run when
//! its size comes. The worker hands back each output's image
//! ([`Shown`]), which the root registers with rinch ([`Registered`]) before
//! each window's style points at its own.

use std::collections::BTreeMap;
use std::path::PathBuf;

use deskrinch_component::LiveSettings;
use deskrinch_settings::wallpaper::WallpaperSettings;
use deskrinch_theme::Theme;
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::picture::{self, Follower, Registered};
use crate::plan::Surface;
use crate::worker::{Shown, Wanted, Worker};
use crate::{role, style, surface};

/// The wallpaper, following `live` and `theme`, with `home` for a picture
/// written under `~/`, until the app ends.
#[component]
pub(crate) fn wallpaper(
    live: LiveSettings<WallpaperSettings>,
    theme: Theme,
    home: Option<PathBuf>,
) -> NodeHandle {
    let verdict = role::of_display();
    if verdict != role::Verdict::Layer {
        role::refuse(verdict);
        // `exit` ends the loop once this returns: no window is opened.
        return rsx! { div {} };
    }
    let _applied = theme.apply();
    let settings = live.into_signal();
    // The picture's path, as the settings name it under this home.
    let path = Memo::new(move || {
        let settings = settings.get();
        settings.image.as_ref()?.expand(home.as_deref())
    });
    // A picture named under `~/` with no home to put it under.
    Effect::new(move || {
        if let Some(image) = settings.get().image
            && path.get().is_none()
        {
            tracing::warn!(
                "wallpaper: cannot show {}: there is no home directory; showing the colour alone",
                image.as_written()
            );
        }
    });
    let (deliver, inbox) = deskrinch_component::handover();
    let shown = inbox.into_signal(Shown::default());
    // First of the effects that read `shown`: the images are registered
    // before any window's style names them.
    let mut registered = Registered::default();
    Effect::new(move || registered.follow(&shown.get()));
    let worker = Worker::start(deliver)
        .inspect_err(|error| {
            tracing::warn!(
                "wallpaper: cannot start its picture's worker: {error}; showing the colour alone"
            );
        })
        .ok();
    let surfaces: Signal<BTreeMap<OutputId, Surface>> = Signal::new(BTreeMap::new());
    if let Some(worker) = worker {
        let mut follower = Follower::default();
        let follows = worker.clone();
        Effect::new(move || follower.follow(path.get().as_deref(), follows.changed()));
        Effect::new(move || {
            worker.want(Wanted {
                path: path.get(),
                fit: settings.get().fit,
                outputs: output_ids().into_iter().collect(),
                surfaces: surfaces.get(),
            });
        });
    }
    rsx! {
        div {
            for id in output_ids() {
                Window {
                    key: id,
                    title: surface::TITLE.to_owned(),
                    width: surface::FIRST_SIZE.0,
                    height: surface::FIRST_SIZE.1,
                    borderless: true,
                    click_through: true,
                    app_id: Some(surface::NAMESPACE.to_owned()),
                    wayland_layer: Some(surface::layer_surface(id)),
                    on_closed: move || {
                        surface::closed(id);
                    },
                    {output(__scope, id, surfaces, shown, settings)}
                }
            }
        }
    }
}

/// What output `id`'s window holds: its surface told to the root while the
/// window lasts, and the element its style fills.
fn output(
    __scope: &mut RenderScope,
    id: OutputId,
    surfaces: Signal<BTreeMap<OutputId, Surface>>,
    shown: Signal<Shown>,
    settings: Signal<WallpaperSettings>,
) -> NodeHandle {
    // Run as this window's code: its own sizes.
    Effect::new(move || {
        if let Some(surface) = this_surface() {
            let known = untracked(|| surfaces.get().get(&id).copied());
            if known != Some(surface) {
                surfaces.update(|all| {
                    all.insert(id, surface);
                });
            }
        }
    });
    __scope.on_cleanup(move || {
        if surfaces.is_alive() {
            surfaces.update(|all| {
                all.remove(&id);
            });
        }
    });
    let source = Memo::new(move || shown.get().names.get(&id).map(|name| picture::source(name)));
    let drawn = Memo::new(move || style::css(&settings.get(), source.get().as_deref()));
    rsx! {
        div { class: "wallpaper", style: {move || drawn.get()} }
    }
}

/// The current window's surface, once rinch has published its size.
fn this_surface() -> Option<Surface> {
    let device = window_physical_size();
    let (width, height) = window_size();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "rinch's logical size is whole pixels, from a u32"
    )]
    let logical = (width.round() as u32, height.round() as u32);
    (device.0 > 0 && device.1 > 0 && logical.0 > 0 && logical.1 > 0)
        .then_some(Surface { device, logical })
}

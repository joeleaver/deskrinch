//! The wallpaper's root: its settings as a signal, the picture they name,
//! and a window on every output.
//!
//! The root is the document of an app with no window of its own
//! (`App::windowless`): it draws nothing, and holds one `Window` per
//! output, a row of a `for` over rinch's `outputs()` keyed by each output's
//! id, which follows the outputs connected, disconnected and changed. A
//! row's window opens with its row and closes with it, or when its output
//! goes first ([`surface::closed`]); an output that only changes (its mode,
//! scale or transform) keeps its row and its window, which the compositor
//! resizes. Before any of it, the root asks whether the display offers a
//! layer shell at all ([`role`]), and opens nothing if it does not.
//!
//! Every window draws the same thing, the [`style`] of the settings, from the same
//! signals: a change of settings repaints every output once, and nothing
//! else does.

use std::path::PathBuf;

use deskrinch_component::LiveSettings;
use deskrinch_settings::wallpaper::WallpaperSettings;
use deskrinch_theme::Theme;
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::picture::{self, Follower};
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
    let mut follower = Follower::default();
    Effect::new(move || follower.follow(path.get().as_deref()));
    let source = Memo::new(move || path.get().map(|path| picture::source(&path)));
    let drawn = Memo::new(move || style::css(&settings.get(), source.get().as_deref()));
    rsx! {
        div {
            for output in outputs() {
                Window {
                    key: output.id,
                    title: surface::TITLE.to_owned(),
                    width: surface::FIRST_SIZE.0,
                    height: surface::FIRST_SIZE.1,
                    borderless: true,
                    click_through: true,
                    app_id: Some(surface::NAMESPACE.to_owned()),
                    wayland_layer: Some(surface::layer_surface(output.id)),
                    on_closed: {
                        let id = output.id;
                        move || {
                            surface::closed(id);
                        }
                    },
                    div { class: "wallpaper", style: {move || drawn.get()} }
                }
            }
        }
    }
}

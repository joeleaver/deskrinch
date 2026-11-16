//! The wallpaper's root: its settings as a signal, the picture they name,
//! and a window on every output.
//!
//! The root is the document of an app with no window of its own
//! (`App::windowless`): it draws nothing, and holds one `Window` per
//! output, a row of a `for` over rinch's `output_ids()`, keyed by the id. A row's window opens with its row and closes with it, when its
//! output is disconnected or turned off, or when its output goes first
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
//! or going does.
//! Nothing a window draws depends on its output. Before any of it, the root
//! asks whether the display offers a layer shell at all ([`role`]), and
//! opens nothing if it does not.
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
                    div { class: "wallpaper", style: {move || drawn.get()} }
                }
            }
        }
    }
}

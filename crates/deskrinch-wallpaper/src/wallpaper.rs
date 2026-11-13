//! The wallpaper's root: its settings as a signal, the picture they name,
//! and a window on every output.
//!
//! The root is the document of an app with no window of its own
//! (`App::windowless`): it draws nothing, and holds one `Window` per
//! output, a row of a keyed `for` over rinch's `outputs()`, which follows
//! the outputs connected, disconnected and changed. A row's window opens
//! with its row and closes with it, or when the compositor closes its
//! surface because its output went; an output that only changes (its
//! mode, scale or transform) keeps its row and its window, which the
//! compositor resizes.
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
    let _applied = theme.apply();
    let settings = live.into_signal();
    // The picture's path, as the settings name it under this home.
    let path = Memo::new(move || {
        let settings = settings.get();
        let image = settings.image.as_ref()?;
        let path = image.expand(home.as_deref());
        if path.is_none() {
            tracing::warn!(
                "wallpaper: cannot show {}: there is no home directory; showing the colour alone",
                image.as_written()
            );
        }
        path
    });
    let mut follower = Follower::default();
    Effect::new(move || follower.follow(path.get().as_deref()));
    let source = Memo::new(move || path.get().map(|path| picture::source(&path)));
    let drawn = Memo::new(move || style::css(&settings.get(), source.get().as_deref()));
    let placed = Memo::new(move || surface::placeable(outputs()));
    // An output without a name gets no window (rinch issue 2658), said
    // once for each such output as it comes.
    let mut reported = 0;
    Effect::new(move || {
        let (_, unnamed) = placed.get();
        if unnamed > reported {
            tracing::warn!(
                "wallpaper: {unnamed} output(s) have no name, so no layer surface can be placed \
                 on them, and they get no wallpaper (rinch issue 2658)"
            );
        }
        reported = unnamed;
    });
    rsx! {
        div {
            for name in placed.get().0 {
                Window {
                    key: name.clone(),
                    title: surface::TITLE.to_owned(),
                    width: surface::FIRST_SIZE.0,
                    height: surface::FIRST_SIZE.1,
                    borderless: true,
                    click_through: true,
                    app_id: Some(surface::NAMESPACE.to_owned()),
                    wayland_layer: Some(surface::layer_surface(name.clone())),
                    on_open: role::watch,
                    div { class: "wallpaper", style: {move || drawn.get()} }
                }
            }
        }
    }
}

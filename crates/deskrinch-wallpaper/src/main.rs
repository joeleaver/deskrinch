//! `deskrinch-wallpaper`: the desktop's wallpaper, a colour or a picture
//! filling the background of every output, below every window and the
//! panel.
//!
//! - One process for all the outputs: an app with no window of its own
//!   (rinch's `App::windowless`) that keeps one window per output (see
//!   [`wallpaper`]), each a `wlr-layer-shell` surface on the background
//!   layer of its output (see [`surface`]). An output connected gets a
//!   window, one disconnected loses it, one resized, rescaled or turned
//!   keeps it at its new size.
//! - What it draws is [`style`]'s CSS: the colour, and the picture fitted
//!   over it; rinch decodes the picture once ([`picture`]) and scales it
//!   to each output when it paints.
//! - It costs nothing while nothing changes: after each output's first
//!   frame it draws again only when the settings, the look or the picture
//!   change, or an output does.
//!
//! # Settings
//!
//! The `[wallpaper]` table of deskrinch's configuration; its keys, values
//! and defaults are documented in `deskrinch-settings`
//! (`deskrinch_settings::wallpaper`), the reference for the configuration
//! file. The wallpaper follows the file while it runs: a change repaints
//! every output, without a restart. Mistakes are reported with their file,
//! line and column, and leave the last good settings in place. A picture
//! that cannot be shown is reported with the path read, `~/` expanded, and
//! the colour shows alone.
//!
//! # Decisions
//!
//! - It takes no keyboard and no pointer: there are no desktop icons and
//!   no menu on the background. A desktop that wants either gives the
//!   wallpaper its pointer then; it is not an oversight.
//! - A compositor without the layer shell (GNOME) has no background layer:
//!   rinch makes such a window an ordinary one, which for a wallpaper would
//!   be a window over everything else. So the wallpaper says why and exits
//!   with a failure status as soon as its first window turns out not to be
//!   a layer surface ([`role`]): rinch cannot yet say whether the layer
//!   shell is there before a window opens (rinch issue 2659).
//! - An output without a name (`wl_output` before version 4) gets no
//!   wallpaper, and the wallpaper says so: no layer surface can be placed
//!   on it (rinch issue 2658).
//!
//! # Look
//!
//! Without a colour of its own, the wallpaper is the look's desktop
//! background (`deskrinch-theme`), dark or light, changing while it runs.
//!
//! # Logging
//!
//! Through `tracing`, set up first (`deskrinch_component::report::init`):
//! `RUST_LOG` picks what is shown, `warn` and worse by default, on standard
//! error, rinch's own messages included.
//!
//! # Rendering
//!
//! rinch's software renderer (tiny-skia), whatever the machine has: a
//! wallpaper draws once per output and then sits still, and the GPU
//! renderer's device and its buffers would cost more memory than the one
//! frame saves. The user overrides that with rinch's `RINCH_RENDERER`
//! environment variable (`gpu`, `software`, `auto`).

mod picture;
mod role;
mod style;
mod surface;
mod wallpaper;

use deskrinch_appearance::Bus;
use deskrinch_component::{Configuration, report};
use deskrinch_settings::wallpaper::WallpaperSettings;
use deskrinch_theme::Theme;
use deskrinch_xdg::BaseDirs;
use rinch::prelude::*;

fn main() {
    // Before the configuration is read, so that its diagnostics are shown.
    report::init();
    let dirs = BaseDirs::from_process();
    let configuration = Configuration::start(&dirs);
    let live = configuration.follow::<WallpaperSettings>();
    let theme = Theme::start(&configuration, Bus::Session);
    let first_theme = theme.initial();
    let home = dirs.home.clone();
    App::windowless(move |scope| wallpaper::wallpaper(scope, live, theme, home))
        .image_scheme(picture::SCHEME, picture::load)
        .renderer(Renderer::Software)
        .theme(first_theme)
        .run();
    if role::refused() {
        std::process::exit(1);
    }
}

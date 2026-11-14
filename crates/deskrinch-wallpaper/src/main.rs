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
//!   be a window over everything else. So the wallpaper asks rinch which
//!   window roles the display offers before opening any window, and
//!   without the layer shell it says why, opens nothing, and exits with a
//!   failure status ([`role`]).
//! - Each output's window is keyed and placed by the output's id, which
//!   every output has: an output without a name (`wl_output` before
//!   version 4) gets its wallpaper like any other ([`surface`]).
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
//! rinch's software renderer (tiny-skia) is the default, for now and for a
//! reason of its own: rinch's GPU renderer presents on the main thread in
//! step with the compositor's frame callbacks, and stalls every window and
//! all event handling when the compositor never shows one of the surfaces
//! (rinch issue 2669, seen under sway with overlapping outputs; its fix is
//! rinch PR 2674, in review). The default is to be chosen from a
//! measurement once the tooling reads rinch's per-window frame lines; until
//! then the hypothesis is that a wallpaper, drawing once per output and
//! then sitting still, costs less in software than a GPU device and its
//! buffers. The user overrides the choice with rinch's `RINCH_RENDERER`
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
        // Software, for now: rinch issue 2669 (see the crate's
        // "Rendering"); chosen again by measurement when the tooling can.
        .renderer(Renderer::Software)
        .theme(first_theme)
        .run();
    if role::refused() {
        std::process::exit(1);
    }
}

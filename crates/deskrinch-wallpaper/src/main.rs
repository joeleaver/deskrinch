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
//! The wallpaper uses rinch's default renderer and adds no choice of its
//! own, as the panel and the launcher do. The build has both of rinch's
//! renderers, the GPU one (Vello on wgpu) and the software one
//! (tiny-skia), and rinch chooses at run time: the GPU, falling back to
//! software with a warning when the GPU will not start, and software where
//! the only adapter is a CPU one (a software Vulkan driver such as
//! lavapipe). The user overrides that with rinch's `RINCH_RENDERER`
//! environment variable (`gpu`, `software` or `cpu`, `auto`).
//!
//! A known issue: with the GPU renderer, rinch issue 2669 can stall the
//! wallpaper, every output's window and all its event handling, under a
//! compositor that stops showing one of its surfaces. It has been seen
//! under sway with two outputs at one position, and labwc is exposed the
//! same way; under KWin with a current Mesa it cannot happen, because the
//! driver paces presents without frame callbacks there. Its fix is rinch
//! PR 2674, and it reaches deskrinch with a later rinch pin.

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
        .theme(first_theme)
        .run();
    if role::refused() {
        std::process::exit(1);
    }
}

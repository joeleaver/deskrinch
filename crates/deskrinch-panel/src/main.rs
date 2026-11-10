//! `deskrinch-panel`: the desktop panel, a bar along an edge of the output
//! that windows keep clear of, with the clock at its right end.
//!
//! - The window is a `wlr-layer-shell` surface (see [`surface`]): the full
//!   width of the output, along its bottom or top edge, reserving its
//!   height. rinch makes it one through `WindowProps::wayland_layer`; on a
//!   compositor without the layer shell (GNOME) rinch falls back to an
//!   ordinary window, with a warning.
//! - The content is the [`bar`]: left, centre and right regions holding
//!   [`widgets`].
//!
//! # Settings
//!
//! The panel reads the `[panel]` table of deskrinch's configuration; its
//! keys, values and defaults are documented in `deskrinch-settings`
//! (`deskrinch_settings::panel`), the reference for the configuration file.
//! The panel follows the file while it runs (`deskrinch-component`'s
//! `LiveSettings`): a change moves or resizes the window, or restarts the
//! clock, without a restart of the panel. Mistakes are reported with their
//! file, line and column, and leave the last good settings in place.
//!
//! # Logging
//!
//! Through `tracing`, set up first (`deskrinch_component::report::init`):
//! `RUST_LOG` picks what is shown, `warn` and worse by default, on standard
//! error, rinch's own messages included.
//!
//! # Rendering
//!
//! The build has both of rinch's renderers: the GPU one (Vello on wgpu)
//! and the software one (tiny-skia). rinch chooses at run time: the GPU,
//! falling back to software with a warning when the GPU will not start.
//! The user overrides that with rinch's `RINCH_RENDERER` environment
//! variable (`gpu`, `software` or `cpu`, `auto`); the panel adds no choice
//! of its own.
//!
//! # Debugging
//!
//! Built with the `debug` feature, the panel runs rinch's debug server, for
//! the rinch MCP tools (screenshots, the DOM, injected input); a normal
//! build has none.

mod bar;
mod panel;
mod surface;
mod widgets;

use deskrinch_component::{LiveSettings, report};
use deskrinch_settings::panel::PanelSettings;
use deskrinch_xdg::BaseDirs;
use rinch::prelude::*;

fn main() {
    // Before the configuration is read, so that its diagnostics are shown.
    report::init();
    let live = LiveSettings::<PanelSettings>::start(&BaseDirs::from_process());
    let window = surface::window_props(surface::Placement::of(&live.initial));
    App::new(move |scope| panel::panel(scope, live))
        .window_props(window)
        .theme(ThemeProviderProps {
            dark_mode: true,
            ..ThemeProviderProps::default()
        })
        .run();
}

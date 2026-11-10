//! `deskrinch-panel`: the desktop panel, a bar along the bottom edge of the
//! output that windows keep clear of, with the clock at its right end.
//!
//! - The window is a `wlr-layer-shell` surface (see [`surface`]): the full
//!   width of the output, 40 logical pixels high, reserving its height.
//!   rinch makes it one through `WindowProps::wayland_layer`; on a
//!   compositor without the layer shell (GNOME) rinch falls back to an
//!   ordinary window, with a warning.
//! - The content is the [`bar`]: left, centre and right regions holding
//!   [`widgets`].
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
mod surface;
mod widgets;

use rinch::prelude::*;

fn main() {
    App::new(bar::bar)
        .window_props(surface::window_props())
        .theme(ThemeProviderProps {
            dark_mode: true,
            ..ThemeProviderProps::default()
        })
        .run();
}

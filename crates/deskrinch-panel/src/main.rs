//! `deskrinch-panel`: the desktop panel, a bar along an edge of the output
//! that windows keep clear of, with the taskbar and, at its end, the clock.
//!
//! - The window is a `wlr-layer-shell` surface (see [`surface`]): the full
//!   width of the output, along its bottom or top edge, reserving its
//!   height. rinch makes it one through `WindowProps::wayland_layer`; on a
//!   compositor without the layer shell (GNOME) rinch falls back to an
//!   ordinary window, with a warning.
//! - The content is the [`bar`]: the taskbar, taking the room the other
//!   [`widgets`] leave, and the end region with the clock.
//!
//! # The taskbar
//!
//! A button for each open window, in the order they opened, with its
//! application's icon and its title cut to fit; the rules are
//! `deskrinch-taskbar`'s, and the widget is [`widgets`]' `taskbar`. The
//! windows come from `deskrinch-window-management`, over a Wayland
//! connection of its own: the wlroots protocol, or KWin's, which KWin
//! offers only to a program it has granted it to by an installed desktop
//! file (`deskrinch-kwin-access`). Without a window list (not granted, or
//! a compositor that offers none) the taskbar is empty, and says why once
//! in the log; so it does when the compositor stops reporting windows. The
//! window list and the installed applications are read on threads of their
//! own, off the first frame's path: the taskbar is empty until the
//! compositor has answered, and its icons follow when the applications are
//! scanned.
//!
//! # Settings
//!
//! The panel reads the `[panel]` table of deskrinch's configuration; its
//! keys, values and defaults are documented in `deskrinch-settings`
//! (`deskrinch_settings::panel`), the reference for the configuration file.
//! The panel follows the file while it runs (`deskrinch-component`'s
//! `Configuration` and `LiveSettings`): a change moves or resizes the
//! window, or restarts the clock, without a restart of the panel. Mistakes are reported with their
//! file, line and column, and leave the last good settings in place.
//!
//! # Look
//!
//! The shell's one look (`deskrinch-theme`): dark or light as `[general]`
//! `color-scheme` says, following the desktop's colour scheme by default,
//! and changing while the panel runs. The panel writes no colour or metric
//! of its own.
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
//! falling back to software with a warning when the GPU will not start,
//! and software where the only adapter is a CPU one (a software Vulkan
//! driver such as lavapipe, as under a compositor that renders with
//! pixman), which would draw more slowly than tiny-skia. The user overrides
//! that with rinch's `RINCH_RENDERER` environment variable (`gpu`,
//! `software` or `cpu`, `auto`): `gpu` insists on the GPU renderer, a CPU
//! adapter included. The panel adds no choice of its own.
//!
//! # Debugging
//!
//! Built with the `debug` feature, the panel can run rinch's debug server,
//! for the rinch MCP tools (screenshots, the DOM, injected input). It is
//! off unless `RINCH_DEBUG=1` is set when the panel starts, and answers
//! only a client holding the token rinch writes, readable by the same user
//! alone. A normal build has none.

mod bar;
mod panel;
mod surface;
mod widgets;

use deskrinch_appearance::Bus;
use deskrinch_applications::{Icons, Installed};
use deskrinch_component::{Configuration, report};
use deskrinch_desktop_entries::Environment;
use deskrinch_icons::SearchPath;
use deskrinch_settings::panel::PanelSettings;
use deskrinch_theme::Theme;
use deskrinch_xdg::BaseDirs;
use rinch::prelude::*;

fn main() {
    // Before the configuration is read, so that its diagnostics are shown.
    report::init();
    let base_dirs = BaseDirs::from_process();
    let configuration = Configuration::start(&base_dirs);
    let live = configuration.follow::<PanelSettings>();
    let theme = Theme::start(&configuration, Bus::Session);
    let window = surface::window_props(surface::Placement::of(&live.initial));
    let first_theme = theme.initial();
    let env = Environment::from_process();
    let parts = panel::Parts {
        live,
        theme,
        installed: Installed::start(&env),
        icons: Icons::start(SearchPath::new(&base_dirs)),
        locales: env.locales,
    };
    App::new(move |scope| panel::panel(scope, parts))
        .window_props(window)
        .theme(first_theme)
        .run();
}

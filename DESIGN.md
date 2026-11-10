# deskrinch — design

A Wayland desktop built on [rinch](https://github.com/joeleaver/rinch).
**Shell first, compositor second.** This file is the canonical design record.

## Why

Desktop environments are built around one UI toolkit (GNOME/GTK, KDE/Qt,
COSMIC/iced). deskrinch is the rinch one. The angle no other desktop has: every
rinch surface already exposes rinch's debug/MCP interface (DOM tree, queries,
input, screenshots), so the desktop and its apps can be inspected and driven by
agents at the structure level, not by pixels or AT-SPI.

## Layers, and who owns what

| Layer | Owns | Phase 1 (on KWin) | Phase 2 |
|---|---|---|---|
| Compositor / window manager | the screen, input routing, **window placement, stacking, tiling, workspaces, focus**, decorations policy | KWin | deskrinch compositor (smithay) |
| Shell | panel, launcher, notifications, OSD, tray, workspace overview, wallpaper | **deskrinch** | deskrinch |
| Session services | settings, lock screen, polkit agent, portals, power, keyring | Plasma's | progressively deskrinch |
| Apps | files, terminal, editor, settings UI | anything | rinch apps preferred |

**Window layout and tiling belong to the compositor.** On Wayland the window
manager *is* the compositor: clients cannot position themselves or each other.
The shell can only see and influence windows through protocols the compositor
chooses to offer:

- KWin: `org_kde_plasma_window_management` (list, activate, minimize, close),
  plus KWin scripting / D-Bus for layout. Tiling stays KWin's in phase 1.
- wlroots compositors: `zwlr_foreign_toplevel_management_v1`.
- Standard, list-only: `ext_foreign_toplevel_list_v1`.

So in phase 1 a deskrinch taskbar can list and activate windows, but tiling is
whatever KWin does. Phase 2 is where layout becomes ours.

## Phase 1 — shell on an existing compositor

Each shell surface is a rinch window with the `wlr-layer-shell` role. KWin and
every wlroots compositor support it; GNOME/Mutter does not (not a target).

### What rinch has today

- rinch's winit fork (`joeleaver/winit-fork`, branch `rinch-patch`) implements
  `LayerShellAttributes` fully: layers `Background`/`Bottom`/`Top`/`Overlay`,
  four-edge `anchor`, `margins`, `exclusive_zone`, keyboard interactivity
  `None`/`OnDemand`/`Exclusive`, `output_name`, `namespace`.
- rinch (`crates/rinch/src/shell/layer_shell.rs`, #1523/#1549) uses it only for
  **overlay windows**: anchor top-left or centred, `exclusive_zone: 0`, layer
  `Top`/`Overlay`. That's enough for popups and OSDs, not for a panel.

### What goes in rinch vs deskrinch: mechanism vs policy

**rinch gets the mechanism**, named after the protocol and never after panels:
a window's Wayland role can be a layer surface with the full set of protocol
parameters. Roughly:

```rust
WindowProps::wayland_layer: Option<LayerSurface {
    layer,           // Background | Bottom | Top | Overlay
    anchor,          // any combination of the four edges
    margins,
    exclusive_zone,  // -1 / 0 / reserve N px
    keyboard,        // None | OnDemand | Exclusive
    output,          // connector name, or compositor's choice
}>
```

Today's `wayland_layer_shell: Some(true)` and `non_activating` stay as
shorthands producing the current overlay configuration. Plus **output
enumeration and hotplug events**, which any multi-monitor app wants.

Why rinch and not deskrinch: rinch already owns the concept
(`rinch_core::element::WindowSurfaceKind::LayerShell`,
`WindowProps::wayland_layer_shell`) and exposes only a fixed slice of it. It
owns everything tied to the window (scale, resize, input, renderer, and the
move/resize gestures in `crates/rinch/src/shell/layer_shell.rs`, which assume a
top-left anchor), so a deskrinch-side override of the winit attributes would
silently break those. Ordinary apps want it too: OSDs, docks, wallpaper,
kiosk/signage, recording indicators, game overlays. Cost: a Wayland-only
public API, falling back to a toplevel on GNOME. rinch already has
per-platform props and already made this commitment for overlays.

**deskrinch keeps the policy**: what a panel is, one per output following
hotplug, which surface reserves which edge, the launcher's keyboard grab,
window-management protocols for the taskbar, notification/tray daemons.

### Window list (taskbar)

Not covered by winit. Needs a separate `wayland-client` connection in the shell
process, speaking `org_kde_plasma_window_management` on KWin and
`zwlr_foreign_toplevel_management_v1` elsewhere, behind one deskrinch trait so
the shell doesn't care which compositor it's on.

### Increments

1. **Panel spike**: one bottom panel, full width, exclusive zone, clock. Runs
   under Plasma. Measure idle CPU and repaint cost of a once-a-second update
   (`perf_stats` over the rinch debug MCP). Open question to settle here:
   does the Vello path re-record the whole scene for a small damaged region?
2. Panel on every output, following hotplug.
3. Launcher (`.desktop` entries, search), `Exclusive` keyboard on open.
4. Taskbar via the window-management trait above.
5. Notifications daemon (`org.freedesktop.Notifications` over D-Bus).
6. OSD (volume/brightness), tray (StatusNotifierItem), wallpaper (`Background`).

## Phase 2 — compositor

Built on **smithay** (MIT). Not forking cosmic-comp, and not pulling in
cosmic-config, cosmic-protocols or libcosmic. cosmic-comp, niri and smithay's
`anvil` example are reference reading only. Keep code that is not ours out of
the tree: cosmic-comp and niri are GPL-3.

Open decision — **where layout lives**: either built into the compositor
(niri/COSMIC style), or an external layout client over a protocol in the style
of river's `river-layout-v3`, so tiling policy is a separate (rinch, or
agent-driven) process and can be swapped without touching the compositor.

## Conventions

- rinch is a git dependency on `main`, never a path dependency; local rinch
  work goes through a gitignored `.cargo/config.toml` `[patch]`. Desktop builds
  must copy rinch's `[patch.crates-io]` wgpu/winit fork block verbatim (see
  rinch-ce's Cargo.toml).
- License MIT OR Apache-2.0.

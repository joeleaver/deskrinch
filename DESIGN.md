# deskrinch — design

A Wayland desktop built on [rinch](https://github.com/joeleaver/rinch).
**Shell first, compositor second.** This file is the canonical design record.

## Why

Desktop environments are built around one UI toolkit (GNOME/GTK, KDE/Qt,
COSMIC/iced). deskrinch is the rinch one. The angle no other desktop has: a
rinch surface can expose rinch's debug/MCP interface (DOM tree, queries, input,
screenshots), so the desktop and its apps can be inspected and driven by agents
at the structure level, not by pixels or AT-SPI. Today that interface is opt-in
and unauthenticated: see "Open decisions".

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
- Standard, list-only: `ext_foreign_toplevel_list_v1` (KWin 6.6.6 does not
  offer it).

So in phase 1 a deskrinch taskbar can list and activate windows, but tiling is
whatever KWin does. Phase 2 is where layout becomes ours.

## Phase 1 — shell on an existing compositor

Each shell surface is a rinch window with the `wlr-layer-shell` role. KWin and
every wlroots compositor support it; GNOME/Mutter does not (not a target).

### What rinch has today

Checked against rinch `main` on 2026-11-10.

- rinch's winit fork (`joeleaver/winit-fork`, branch `rinch-patch-beta4`,
  pinned by rev in rinch's `Cargo.toml`) implements `LayerShellAttributes`
  fully: layers `Background`/`Bottom`/`Top`/`Overlay`, four-edge `anchor`,
  `margins`, `exclusive_zone`, keyboard interactivity
  `None`/`OnDemand`/`Exclusive`, `output_name`, `namespace`. It has runtime
  setters for margins, anchor, layer, keyboard interactivity and exclusive
  zone. There is none for the output: the protocol fixes it at creation, so
  moving a surface to another output means recreating it.
- rinch (`crates/rinch/src/shell/layer_shell.rs`, #1523/#1549) uses it only for
  **overlay windows**: anchor top-left or centred, `exclusive_zone: 0`, layer
  `Top`/`Overlay`. That's enough for popups and OSDs, not for a panel.
- **Outputs.** rinch calls winit's `available_monitors` internally and has no
  public enumeration API. The fork tracks outputs as they come and go (sctk's
  `OutputHandler`) but sends the application no event, so hotplug needs a fork
  change or polling.
- **One window per process.** The runtime holds one app window (plus
  DevTools). `rinch::windows::open_window` queues a request that nothing
  consumes.
- **Debug/MCP interface.** Behind `features = ["debug"]`: a TCP server on a
  `127.0.0.1` port, with no authentication.

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

Size is part of the role: an axis anchored to both edges can be left to the
compositor (the protocol's size 0), which is how a full-width bar gets its
width. The API has to say how an app asks for that.

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

KWin offers `org_kde_plasma_window_management` only to a client whose installed
`.desktop` file lists it in `X-KDE-Wayland-Interfaces` (KWin 6.6.6: an ordinary
client is not offered the global). So the taskbar needs deskrinch installed
with a desktop file. How KWin matches a running client to that file, and so
how a development build gets access, is to be confirmed in that increment.

### Running beside plasmashell

Layer surfaces coexist with Plasma's: two panels can both be on screen. Session
bus names do not, since each has one owner. In the live session plasmashell
owns `org.freedesktop.Notifications` and kded6 owns
`org.kde.StatusNotifierWatcher`, so the notification daemon and the tray host
replace Plasma's rather than run beside them.

### Repaint cost

rinch's GPU window already repaints only its damage (rinch #1718,
`docs/internals/paint-and-damage.md`): the scene is encoded pruned to the
damage, then rendered into a target that reaches from the surface origin to
the damage's far corner. So a clock at the right end of a panel renders a
target nearly the panel's size. Increment 0 measures that case.

### Increments

0. **Measurement spike** on today's rinch: a strip with a clock on the existing
   overlay role (no exclusive zone). Measure idle CPU and wakeups, the cost of
   a once-a-second repaint (`perf_stats` over the rinch debug MCP), and memory,
   on both renderers. Settles whether rinch is cheap enough to be an always-on
   shell before the rinch work is paid for.
1. **Panel**: bottom, full width, exclusive zone, clock. Runs under Plasma.
   Needs the full layer-surface role in rinch.
2. Panel on every output, following hotplug.
3. Launcher (`.desktop` entries, search), `Exclusive` keyboard on open.
4. Taskbar via the window-management trait above.
5. Notifications daemon (`org.freedesktop.Notifications` over D-Bus).
6. OSD (volume/brightness), tray (StatusNotifierItem), wallpaper (`Background`).

## Open decisions

- **Process model.** One process per shell surface, or one shell process with
  many surfaces. rinch has one window per process today, so the second needs
  multi-window in rinch (mechanism, upstream). It affects memory (a renderer
  per process), crash isolation, and how the panel per output is built. Decide
  before increment 2.
- **Agent interface.** The debug/MCP interface is the project's angle, and
  today it is an unauthenticated localhost port that can screenshot a surface
  and inject input. A shell that runs with it on needs a security model: who
  may connect, and how that is granted. The mechanism is rinch's.
- **What ends phase 1.** Replace plasmashell for daily use, or run beside it.
  The workspace overview is in the shell's row of the table above and in no
  increment yet (KWin offers `org_kde_plasma_virtual_desktop_management` to an
  ordinary client).
- **Where layout lives** (phase 2, below).

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
  work goes through a gitignored `.cargo/config.toml` `[patch]`. rinch's forks
  (winit, Vello, Parley, Taffy) are its own pinned git dependencies and come
  with it: no `[patch.crates-io]` lines here. rinch-ce's `Cargo.toml` still has
  a wgpu patch block from before rinch #1955; don't copy it.
- License MIT OR Apache-2.0.

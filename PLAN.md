# deskrinch — plan

Work order. [DESIGN.md](DESIGN.md) says *why*; this file says *what next*.
Update the **Status** line and tick items as they land.

**Status (2026-11-10):** design reviewed against rinch `main`, no code. Next:
step 0, and the two step 1 issues. Three decisions are open (below).

Not blocked on rinch, so they can run beside step 1: step 0, and the "core"
item of steps 4, 5 and 6.

## Decisions (see DESIGN.md "Open decisions")

- [ ] Process model: one process per surface, or multi-window in rinch.
      Needed before step 3.
- [ ] What ends phase 1: replace plasmashell, or run beside it.
- [ ] Security model for the debug/MCP interface. Needed before a shell with
      `debug` on runs as the daily desktop.

## Step 0 — measurement spike (today's rinch)

Settles whether rinch is cheap enough to be an always-on shell before step 1
is paid for.

- [ ] Cargo workspace here; rinch as a git dep on `main`. No
      `[patch.crates-io]` lines (DESIGN.md "Conventions").
- [ ] `deskrinch-panel` spike: a 40 px strip with a clock along the bottom of
      the output, on rinch's existing overlay role
      (`wayland_layer_shell: Some(true)`, positioned; no exclusive zone, so
      windows go under it).
- [ ] Measure, on the GPU and the software renderer: idle CPU and wakeups, the
      cost of the once-a-second repaint (`perf_stats` over the rinch debug MCP
      and the process's CPU time), and resident memory. Put the clock at the
      right end: that is the largest partial-frame target (DESIGN.md "Repaint
      cost").
- [ ] Record the numbers in DESIGN.md. Anything out of line is a rinch issue.

## Step 1 — rinch: layer-surface role and outputs (upstream, in `joeleaver/rinch`)

The mechanism half of DESIGN.md "mechanism vs policy". Done in the rinch repo,
following rinch's own `AGENTS.md`/`CLAUDE.md` process, not here. Two issues and
two PRs: 1a and 1b touch different code.

### 1a — full layer-surface role

- [ ] Open a rinch issue: "Wayland layer surfaces: expose the full
      wlr-layer-shell role (layer, anchors, margins, exclusive zone, keyboard,
      output, compositor-assigned size)". Reference #1523 (overlays) and
      #1604 (open resize-overshoot follow-up, same module).
- [ ] `rinch-core`: `WindowProps::wayland_layer: Option<LayerSurface>` with the
      fields in DESIGN.md. Keep `wayland_layer_shell` / `non_activating` as
      shorthands for today's overlay configuration; existing behaviour and
      tests unchanged.
- [ ] `crates/rinch/src/shell/layer_shell.rs`: map `LayerSurface` onto the winit
      fork's `LayerShellAttributes` (already complete: `Background`/`Bottom`/
      `Top`/`Overlay`, four-edge anchor, margins, exclusive zone, keyboard
      `None`/`OnDemand`/`Exclusive`, `output_name`, namespace). Move/resize
      gestures currently assume a top-left anchor: define their behaviour for
      other anchors (likely: disabled when the app supplied an explicit
      anchor), don't leave it undefined.
- [ ] Size: an axis anchored to both edges takes the compositor's size (the
      protocol's size 0), which is how a bar gets the output's full width.
      Define how an app asks for it. The fork's `configure_layer` takes the
      compositor's size when it gives both axes; whether a surface can be
      created asking for 0 on an axis is to be checked.
- [ ] Runtime updates: exclusive zone, margins, anchor, layer and keyboard
      change reactively, over the fork's existing `set_layer_shell_*` setters.
      The output is fixed at creation: a change of output recreates the
      surface.
- [ ] Example in rinch: a full-width bottom bar with an exclusive zone, plus a
      `Background` wallpaper surface. Verify under KWin (nested
      `kwin_wayland`, then the live Plasma session) and on a wlroots
      compositor (sway, nested). sway is not installed on the dev machine yet.
- [ ] Docs: rinch's `docs/internals/app-and-windows.md` and its user guide.

### 1b — outputs

- [ ] Open a rinch issue: "Output enumeration and hotplug".
- [ ] Enumeration: a public API listing outputs (connector name, logical size,
      scale), so an app can open one surface per monitor. rinch calls winit's
      `available_monitors` internally only.
- [ ] Hotplug: an added/removed signal. The fork tracks outputs (sctk's
      `OutputHandler`) but sends the application no event: add one to the
      fork, or justify polling. Generic, not Wayland-only, where the platform
      allows.
- [ ] Verify with more than one output. The dev machine has one (DP-1), so
      use a nested `kwin_wayland` with `--output-count`.

## Step 2 — panel (needs 1a)

- [ ] `deskrinch-panel`: bottom, full width, ~40 px exclusive zone, clock.
      Runs under the current Plasma session alongside plasmashell.
      `.cargo/config.toml` `[patch]` to a local rinch checkout while 1a is
      unmerged.

## Step 3 — multi-output panel (needs 1b and the process-model decision)

- [ ] One panel per output, following hotplug.

## Step 4 — launcher

- [ ] Core, no rinch: `.desktop` entry discovery, search, and launching by the
      desktop-entry `Exec` rules. A library crate with tests.
- [ ] UI: `Exclusive` keyboard while open (needs 1a).

## Step 5 — taskbar

- [ ] Core, no rinch: a deskrinch `WindowManagement` trait over a separate
      `wayland-client` connection: `org_kde_plasma_window_management` (KWin)
      and `zwlr_foreign_toplevel_management_v1` (wlroots). List, activate,
      minimize, close.
- [ ] KWin access: the protocol is offered only to a client whose installed
      `.desktop` file lists it in `X-KDE-Wayland-Interfaces`. Install a
      deskrinch desktop file, and confirm how KWin matches a development
      build to it.
- [ ] Taskbar in the panel.

## Step 6 — notifications, OSD, tray, wallpaper

Notifications and the tray replace Plasma's rather than run beside them: each
bus name has one owner (DESIGN.md "Running beside plasmashell").

- [ ] Core, no rinch: `org.freedesktop.Notifications` daemon (D-Bus), the
      service and its model. Tested on a private session bus.
- [ ] Notification surfaces: Overlay layer, top-right anchor (needs 1a).
- [ ] Volume/brightness OSD.
- [ ] StatusNotifierItem tray host.
- [ ] Wallpaper on the `Background` layer.

## Later — phase 2 compositor (smithay)

Not planned in detail yet. First decide where layout lives (built-in vs
river-layout-style external client, DESIGN.md).

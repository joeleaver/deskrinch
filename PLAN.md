# deskrinch — plan

Work order. [DESIGN.md](DESIGN.md) says *why*; this file says *what next*.
Update the **Status** line and tick items as they land.

**Status (2026-11-10):** design written, no code. Next: step 1.

## Step 1 — rinch: full layer-surface API (upstream, in `joeleaver/rinch`)

The mechanism half of DESIGN.md "mechanism vs policy". Done in the rinch repo,
following rinch's own `AGENTS.md`/`CLAUDE.md` process, not here.

- [ ] Open a rinch issue: "Wayland layer surfaces: expose the full
      wlr-layer-shell role (layer, anchors, margins, exclusive zone, keyboard,
      output) + output enumeration/hotplug". Reference #1523 (overlays) and
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
- [ ] Runtime updates: changing exclusive zone / margins / anchor / size
      reactively (the fork already has `set_layer_shell_exclusive_zone` etc.;
      check what else it exposes and extend the fork if needed).
- [ ] Output enumeration + hotplug: list outputs (connector name, logical
      size, scale) and an added/removed signal, so an app can open one surface
      per monitor. Generic, not Wayland-only, where the platform allows.
- [ ] Example in rinch: a full-width bottom bar with an exclusive zone, plus a
      `Background` wallpaper surface. Verify under KWin (Plasma) and on a
      wlroots compositor (sway in a nested session is fine).
- [ ] Docs in rinch's internals docs for the new API.

## Step 2 — deskrinch panel spike

- [ ] Cargo workspace here; rinch as a git dep on `main`, rinch's
      `[patch.crates-io]` wgpu/winit block copied verbatim (see
      `~/dev/rinch-ce/Cargo.toml`); `.cargo/config.toml` `[patch]` for a local
      rinch checkout while step 1 is unmerged.
- [ ] `deskrinch-panel`: bottom, full width, ~40 px exclusive zone, clock.
      Runs under the current Plasma session alongside plasmashell.
- [ ] Measure: idle CPU and per-tick repaint cost of the once-a-second clock,
      via `perf_stats` over the rinch debug MCP and the process's CPU time.
      Settle the open question: does the Vello path re-record the whole scene
      for a small damaged region? If it does, that's a rinch issue.

## Step 3 — multi-output panel

- [ ] One panel per output, following hotplug (step 1's enumeration).

## Step 4 — launcher

- [ ] `.desktop` entry discovery + search; `Exclusive` keyboard while open;
      launches via the desktop-entry `Exec` rules.

## Step 5 — taskbar

- [ ] A deskrinch `WindowManagement` trait over a separate `wayland-client`
      connection: `org_kde_plasma_window_management` (KWin) and
      `zwlr_foreign_toplevel_management_v1` (wlroots). List, activate,
      minimize, close.

## Step 6 — notifications, OSD, tray, wallpaper

- [ ] `org.freedesktop.Notifications` daemon (D-Bus), Overlay layer,
      top-right anchor.
- [ ] Volume/brightness OSD.
- [ ] StatusNotifierItem tray host.
- [ ] Wallpaper on the `Background` layer.

## Later — phase 2 compositor (smithay)

Not planned in detail yet. First decide where layout lives (built-in vs
river-layout-style external client, DESIGN.md).

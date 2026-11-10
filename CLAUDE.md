# deskrinch — agent notes

A rinch-based Wayland desktop: shell first (layer-shell surfaces under KWin),
then a smithay compositor.

Start every session by reading:
1. [PLAN.md](PLAN.md): current status and the next unchecked step.
2. [DESIGN.md](DESIGN.md): canonical design. Change it when a decision changes.

Rules:
- **Mechanism vs policy.** Generic windowing capability (layer surfaces,
  outputs) is fixed in rinch (`~/dev/rinch`, `joeleaver/rinch`), following
  that repo's own AGENTS.md/CLAUDE.md. Never work around a rinch gap from
  here; desktop policy (what a panel is, per-output layout, daemons) lives
  here.
- rinch is a git dependency on `main`, never a path dependency. Local rinch
  work goes through the gitignored `.cargo/config.toml` `[patch]`.
- Phase 2 builds on smithay only. cosmic-comp and niri are GPL-3 reference
  reading; don't copy code from them.
- Correctness over shortcuts; no tech debt.
- Keep PLAN.md's Status line and checkboxes current as work lands.

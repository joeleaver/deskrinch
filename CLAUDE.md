# deskrinch — agent notes

A rinch-based Wayland desktop: shell first (layer-shell surfaces under KWin),
then a smithay compositor. Written in Rust.

## Where things live

- **Plan, design, decisions, knowledge base and tracking** are in Pimble, at
  `Personal/deskrinch`, read through the Pimble MCP tools. Start with its
  Board (what is in flight and what is next) and its Design. It is the only
  copy: don't recreate plan or design files in this repository.
- **This repository** holds the code, its rustdoc, the README and these rules.
- **GitHub issues** on deskrinch are for issues filed from outside. Internal
  work is tracked on the Pimble Board, not as issues.

## How work is run

One session is the project manager: it makes the decisions, briefs the work,
reviews every diff, merges, and keeps Pimble current. Implementation is done
by Opus 5.5 agents at medium effort, one Board card each.

If you were dispatched with a brief: do that card, report what you did and
what you could not verify, and raise a design question instead of deciding
it. Only the project manager writes to Pimble.

## Rules

- **Rust is the language**, tooling included: repository tasks are
  `cargo xtask` subcommands, not shell or Python scripts.
- **No tech debt, ever.** Do it right the first time, even when that means
  rework now or pausing to build a dependency first. Time and cost are not
  constraints. No stubs, no TODOs left behind, nothing "for now".
- **Small files.** No source file much above 500 lines; CI fails at 600.
  Split by responsibility before a file gets there.
- **Don't repeat yourself.** Shared code goes in a library crate, and one
  fact has one home.
- **Best practice is the default:** `cargo fmt`, clippy clean with warnings
  denied, tests in the same change as the code, rustdoc on public items,
  errors as typed values, no `unsafe` without a written safety argument.
- **Mechanism vs policy.** Generic windowing capability (layer surfaces,
  outputs, several windows in a process) belongs in rinch. Never work around
  a rinch gap from here; desktop policy (what a panel is, per-output layout,
  daemons) lives here.
- **rinch is changed only through PRs.** The local checkout at `~/dev/rinch`
  is read-only from here. Work in a fresh clone under the gitignored `.work/`,
  follow rinch's own `AGENTS.md`/`CLAUDE.md`, and open a PR against
  `joeleaver/rinch`. The same goes for rinch's forks. An agent stops at an
  open PR with its checks green, and the project manager reviews it.
- **rinch's own session merges and releases rinch.** Nothing is merged into
  rinch or its forks from here, and no batch is opened from here. The project
  manager hands each reviewed PR to the rinch session, saying what was run and
  what was not, and tells it of anything our work meets on its side (a shared
  pin, a file both touch). This session's work is the shell.
- rinch is a git dependency on `main`, never a path dependency, and needs no
  `[patch.crates-io]` lines. Unmerged rinch work is tested through a
  `[patch]` in the gitignored `.cargo/config.local.toml`, which the tracked
  `.cargo/config.toml` includes.
- **Test sessions are isolated from the real one.** A nested compositor, a
  test D-Bus bus, or anything that talks to session services runs only inside
  an isolated session: its own `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`,
  `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_STATE_HOME` under a temporary
  directory, the real session's display and bus variables removed, a private
  bus with service activation off (`crates/deskrinch-test-dbus`), and its own
  process group, ended as a whole afterwards. Never plain `dbus-run-session`:
  its default configuration activates the desktop portals and the secret
  service, which then run against the real session.
  `cargo xtask session <kwin|sway|labwc> <program>` runs any command in such
  a session and is the way to do it; its code is `xtask/src/session/` on
  `crates/deskrinch-test-dbus`. In a rinch clone, rinch's own
  `scripts/isolated-session.sh` does the same. Nothing is
  installed, written or launched in the user's real session or home. This
  holds for work in rinch clones too.
- **Temporary files are your own.** Agents share scratch directories. Never
  write a fixed file name there (a PR description, a script, a log): make a
  directory with `mktemp -d` and work in it.
- Phase 2 builds on smithay only. cosmic-comp and niri are GPL-3 reference
  reading; don't copy code from them.
- Code reaches `main` through a pull request with CI green.

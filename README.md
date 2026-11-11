# deskrinch

A Wayland desktop built on [rinch](https://github.com/joeleaver/rinch): a shell
first (panel, launcher, notifications on `wlr-layer-shell`, running under KWin),
then a smithay-based compositor.

Early days: the libraries the shell rests on, and a panel (`deskrinch-panel`).

The configuration file, `~/.config/deskrinch/config.toml`, is documented in
the `deskrinch-settings` crate (`cargo doc -p deskrinch-settings --open`).

## Building and checking

`rust-toolchain.toml` pins the compiler; rustup installs it on first use. CI
runs exactly these, and a pull request merges only when all pass:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny --locked check      # cargo install cargo-deny --locked
cargo xtask file-length
```

Building rinch, which the panel uses, needs the GTK 3 and libxdo development
files for its native menus (on Debian and Ubuntu, `libgtk-3-dev` and
`libxdo-dev`).

The desktop-entries crate's tests start a private `dbus-daemon` (from the
`dbus` package) for the systemd-scope and D-Bus activation paths; it talks
to nothing in your session.

The notification service's tests also start private D-Bus daemons, and run
libnotify's `notify-send` against them (on Debian and Ubuntu, the
`libnotify-bin` package). They never touch the session bus.

The appearance crate's tests run a stand-in settings portal on a private
D-Bus daemon too, never the session's.

The window-management crate's compositor tests drive a real compositor and
are ignored by `cargo test`. This runs them in nested, headless KWin, sway and
labwc sessions that touch nothing in your own (the isolated sessions
`cargo xtask session` gives; it needs `dbus-daemon`, `kwin_wayland`, `sway` and
`labwc`; name compositors to run only those):

```sh
cargo xtask compositor-tests [kwin] [sway] [labwc]
```

One desktop-entries test launches into a real transient scope of your
systemd user instance, and is ignored by `cargo test`. It starts only its
own recorder script, and stops the scope again:

```sh
cargo xtask systemd-scope-tests
```

### Running something under a nested compositor

`cargo xtask session` is the one way to run anything under a nested
compositor (a shell component, a rinch example, a screenshot tool): never a
hand-made session. It starts a fresh headless KWin, sway or labwc, runs the
command as its client, and when the command exits ends the whole session:

```sh
cargo xtask session sway sh -c 'foot & sleep 2; grim shot.png'
cargo xtask session --outputs 2 kwin sh -c 'foot --maximized & sleep 3; spectacle -b -n -f -o shot.png'
```

What the command, and everything it starts, sees:

- `HOME`, `XDG_RUNTIME_DIR`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`,
  `XDG_CACHE_HOME` and `XDG_STATE_HOME`: private directories under a fresh
  `/tmp/deskrinch-session-*` (also `$DESKRINCH_SESSION_DIR`).
- `XDG_CONFIG_DIRS=/etc/xdg` and `XDG_DATA_DIRS=/usr/local/share/:/usr/share/`,
  the XDG specification's defaults, not yours.
- `CARGO_HOME` and `RUSTUP_HOME` at your real ones, so cargo and rustup work.
- `WAYLAND_DISPLAY` naming the nested compositor (and `SWAYSOCK` its IPC
  under sway). There is no X server: KWin runs without Xwayland.
- `DBUS_SESSION_BUS_ADDRESS` naming a private `dbus-daemon` with service
  activation off, which the compositor uses too: no portal, secret service
  or notification daemon is ever started on demand.
- `XDG_CURRENT_DESKTOP` only when `--current-desktop NAME` sets it.
- None of your `DISPLAY`, `XAUTHORITY`, `WAYLAND_DISPLAY`, `WAYLAND_SOCKET`,
  `SWAYSOCK`, `I3SOCK`, `DBUS_SESSION_BUS_ADDRESS`, `XDG_CURRENT_DESKTOP`, or
  the login session's `XDG_SESSION_ID`, `XDG_SESSION_CLASS`,
  `XDG_SESSION_DESKTOP`, `XDG_SESSION_TYPE`, `XDG_SESSION_PATH`, `XDG_SEAT`,
  `XDG_SEAT_PATH` and `XDG_VTNR`.
- Everything else as you have it (`PATH`, the locale, `TERM`, `CARGO_*` and
  `RUST*` settings, ...).

Caches follow `XDG_CACHE_HOME` and `HOME`, so a GPU client's shader cache
(Mesa's `mesa_shader_cache`, for one) is inside the session and starts empty
each time; set `MESA_SHADER_CACHE_DIR` in the command to keep one.

- Standard input closed, and the working directory you called it from.
- One process group with the compositor. When the command exits, or the task
  gets `SIGINT`, `SIGTERM` or `SIGHUP`, the group gets `SIGTERM`, then
  `SIGKILL`, and the bus is stopped. A process that escaped the group is then
  found by its environment, killed, and reported as a failure.

It exits with the command's status (128 + N for signal N), 130 when
interrupted, and 70 when the session could not be set up or did not end
cleanly. `--outputs N` sets the number of outputs, `--keep` keeps the
session's directory (with the compositor's log); `cargo xtask session --help`
has the details.

From outside this repository, for example in a rinch clone under `.work/`,
name xtask's manifest; the command still runs in your directory:

```sh
cargo run -q --manifest-path ~/dev/deskrinch/xtask/Cargo.toml -- session sway sh -c '...'
```

Cargo takes its toolchain from the directory it runs in. A rinch checkout
pins the same toolchain as deskrinch, so this works there; elsewhere, add
`+<channel>` with the channel deskrinch's `rust-toolchain.toml` names.

Its own tests start only isolated sessions; they need a compositor, so they
are ignored by `cargo test`:

```sh
DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test session -- --ignored
```

### Measuring what a component costs

`cargo xtask measure` starts a rinch program (a shell component, a rinch
example) in fresh isolated sessions, one per run, and reports what it costs:

```sh
cargo build --release -p deskrinch-panel
cargo xtask measure target/release/deskrinch-panel
cargo xtask measure --shader-cache warm --renderer gpu target/release/deskrinch-panel
cargo xtask measure --instructions --runs 1 target/release/deskrinch-panel
```

For each run: the CPU time to the first presented frame, in all and per
thread (shader-compiling workers that have ended included); the memory once
settled, with the resident set grouped by what is mapped (the Vulkan driver,
LLVM, fonts, GTK, the binary, the heap) and the GPU memory the DRM driver
reports, which is not in the resident set; over an idle window (125 s, two
ticks of a minute clock), CPU time, wake-ups and preemptions per second,
frames and CPU per frame; and the GPU adapter wgpu chose, marked when it is a
software Vulkan driver. Across runs it prints each number's median and range.

The figures to compare are CPU time and, with `--instructions` (the program
under Valgrind's Callgrind; needs `valgrind`), instruction counts, which do
not move with the machine's load. Wall time is printed apart, beside the load
average, as not comparable.

Each session's shader cache starts empty (`--shader-cache cold`, a first
start); `--shader-cache warm` fills one with an unmeasured start first, as
every later start in a real session finds it. Measure GPU costs under KWin
(`--compositor kwin`, the default), which composites with the GPU: headless
sway and labwc render with pixman and offer no dmabuf, so wgpu falls back to a
software Vulkan driver there. `cargo xtask measure --help` has the details.

Its end-to-end tests measure stand-in scripts in isolated sessions, and are
ignored by `cargo test` like the session's (they do not cover
`--instructions`, which needs Valgrind):

```sh
DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure -- --ignored
```

Repository tasks are `cargo xtask` subcommands; `cargo xtask --help` lists
them. Local Cargo settings, such as a `[patch]` for unmerged rinch work, go in
the gitignored `.cargo/config.local.toml`.

Licensed under either of MIT or Apache-2.0, at your option.

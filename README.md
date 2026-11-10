# deskrinch

A Wayland desktop built on [rinch](https://github.com/joeleaver/rinch): a shell
first (panel, launcher, notifications on `wlr-layer-shell`, running under KWin),
then a smithay-based compositor.

Early days. No desktop code yet.

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

The desktop-entries crate's tests start a private `dbus-daemon` (from the
`dbus` package) for the systemd-scope and D-Bus activation paths; it talks
to nothing in your session.

The notification service's tests also start private D-Bus daemons, and run
libnotify's `notify-send` against them (on Debian and Ubuntu, the
`libnotify-bin` package). They never touch the session bus.

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

What the command, and everything it starts, gets:

- Private `XDG_RUNTIME_DIR`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`,
  `XDG_CACHE_HOME` and `XDG_STATE_HOME` under a fresh
  `/tmp/deskrinch-session-*` (also `$DESKRINCH_SESSION_DIR`); `HOME` is
  unchanged.
- None of your session's `DISPLAY`, `XAUTHORITY`, `WAYLAND_DISPLAY`,
  `WAYLAND_SOCKET`, `SWAYSOCK`, `I3SOCK` or `DBUS_SESSION_BUS_ADDRESS`.
  `WAYLAND_DISPLAY` names the nested compositor (and `SWAYSOCK` its IPC under
  sway). There is no X server: KWin runs without Xwayland.
- A private `dbus-daemon` with service activation off, in
  `DBUS_SESSION_BUS_ADDRESS`, which the compositor uses too: no portal,
  secret service or notification daemon is ever started on demand.
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

Repository tasks are `cargo xtask` subcommands; `cargo xtask --help` lists
them. Local Cargo settings, such as a `[patch]` for unmerged rinch work, go in
the gitignored `.cargo/config.local.toml`.

Licensed under either of MIT or Apache-2.0, at your option.

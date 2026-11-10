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
labwc sessions that touch nothing in your own (it needs `kwin_wayland`,
`dbus-run-session`, `sway` and `labwc`; name compositors to run only those):

```sh
cargo xtask compositor-tests [kwin] [sway] [labwc]
```

One desktop-entries test launches into a real transient scope of your
systemd user instance, and is ignored by `cargo test`. It starts only its
own recorder script, and stops the scope again:

```sh
cargo xtask systemd-scope-tests
```

To try a shell component by hand, run it in such a session rather than in
your own: the command is the session's client, and when it exits the session
ends with everything it started.

```sh
cargo xtask session sway sh -c 'foot & sleep 2; grim shot.png'
cargo xtask session kwin sh -c 'foot --maximized & sleep 3; spectacle -b -n -f -o shot.png'
```

Repository tasks are `cargo xtask` subcommands; `cargo xtask --help` lists
them. Local Cargo settings, such as a `[patch]` for unmerged rinch work, go in
the gitignored `.cargo/config.local.toml`.

Licensed under either of MIT or Apache-2.0, at your option.

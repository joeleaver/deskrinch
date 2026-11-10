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
are ignored by `cargo test`. This runs them in nested, headless KWin and sway
sessions that touch nothing in your own (it needs `kwin_wayland`,
`dbus-run-session` and `sway`; name one compositor to run only that):

```sh
cargo xtask compositor-tests [kwin] [sway]
```

Repository tasks are `cargo xtask` subcommands; `cargo xtask --help` lists
them. Local Cargo settings, such as a `[patch]` for unmerged rinch work, go in
the gitignored `.cargo/config.local.toml`.

Licensed under either of MIT or Apache-2.0, at your option.

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

Repository tasks are `cargo xtask` subcommands; `cargo xtask --help` lists
them. Local Cargo settings, such as a `[patch]` for unmerged rinch work, go in
the gitignored `.cargo/config.local.toml`.

Licensed under either of MIT or Apache-2.0, at your option.

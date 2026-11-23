//! The cargo xtask runs: `CARGO` from the environment, else `cargo` on
//! `PATH`. Never a path compiled into xtask (`env!("CARGO")`).
//!
//! The toolchain `rust-toolchain.toml` pins reaches every compile only
//! through rustup's proxy: started as `cargo`, the proxy reads the pin and
//! sets `RUSTUP_TOOLCHAIN` for everything below it. Cargo runs rustc for a
//! registry crate from that crate's directory, where there is no pin, so
//! without `RUSTUP_TOOLCHAIN` the `rustc` proxy there answers with rustup's
//! default toolchain, and registry crates are built by another compiler
//! into the same build directory. A compiled-in path names the toolchain's
//! own cargo and skips the proxy: when xtask was not itself started by
//! cargo (a test binary run directly, say), nothing above it set
//! `RUSTUP_TOOLCHAIN` and nothing below does. `CARGO` is set only by a
//! cargo that runs xtask, and so only alongside `RUSTUP_TOOLCHAIN`; when
//! it is not set, `cargo` on `PATH` is the proxy.
//!
//! One case is not covered, and is not xtask's to cover: someone who starts
//! the toolchain's cargo by its absolute path themselves, so that `CARGO`
//! is set without `RUSTUP_TOOLCHAIN`.

use std::ffi::OsString;
use std::process::Command;

/// A command running the cargo to run, as the module's doc says.
pub(crate) fn command() -> Command {
    Command::new(program(std::env::var_os("CARGO")))
}

/// The cargo to run, given `CARGO`'s value.
fn program(from_environment: Option<OsString>) -> OsString {
    from_environment.unwrap_or_else(|| "cargo".into())
}

#[cfg(test)]
mod tests;

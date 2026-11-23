//! `cargo xtask busy-compositor`: keeps the session's compositor busy with
//! a fixed amount of plain Wayland work, then says when the compositor had
//! answered all of it. For measure's tests: a settle that waits for the
//! compositor must not end while the compositor still works for this
//! client.
//!
//! It opens a window, waits until the compositor has mapped it, and closes
//! it, `--rounds` times; then opens one more and waits until that is
//! mapped, which the compositor does only after every request before it.
//! At that moment it writes the time (nanoseconds since the Unix epoch,
//! the clock `date +%s%N` reads) into `--done`. All of it must be done
//! within `--within` seconds, or it fails saying the compositor never
//! finished.
//!
//! Hidden from `--help`: a test helper, refused outside a nested session.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use deskrinch_test_client::{Compositor, NotNested, Paint, TestClient};
use deskrinch_test_wayland::Rgba;

use crate::Outcome;
use crate::error::Error;

/// `cargo xtask busy-compositor`'s arguments.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// How many windows to open, have mapped and close.
    #[arg(long)]
    rounds: u32,
    /// The file the time the compositor had answered everything is written
    /// to.
    #[arg(long)]
    done: PathBuf,
    /// The most seconds all of it may take.
    #[arg(long)]
    within: u64,
}

/// Why it failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BusyError {
    /// It was run outside a nested session.
    #[error(
        "`cargo xtask busy-compositor` runs only in a nested session xtask started, as \
         outside one it would load the real desktop: {0}"
    )]
    NotNested(NotNested),
    /// A window could not be opened or mapped.
    #[error("the busy client's window")]
    Client(#[from] deskrinch_test_client::Error),
    /// The work was not done in time.
    #[error(
        "the compositor never finished the busy client's {rounds} round(s) within {within} s: \
         {done} done"
    )]
    NeverFinished {
        /// How many rounds were asked for.
        rounds: u32,
        /// How many were done.
        done: u32,
        /// The limit, in seconds.
        within: u64,
    },
    /// The done file could not be written.
    #[error("write the time the busy client was answered")]
    Io(#[from] std::io::Error),
}

/// Does the work, then writes the time it was all answered.
pub(crate) fn run(args: &Args) -> Result<Outcome, Error> {
    busy(args)?;
    Ok(Outcome::Passed)
}

fn busy(args: &Args) -> Result<(), BusyError> {
    Compositor::in_session().map_err(BusyError::NotNested)?;
    let limit = Instant::now() + Duration::from_secs(args.within);
    let client = TestClient::connect()?;
    let paint = || Paint::Solid(Rgba::opaque(40, 40, 40));
    for done in 0..args.rounds {
        if Instant::now() >= limit {
            return Err(BusyError::NeverFinished {
                rounds: args.rounds,
                done,
                within: args.within,
            });
        }
        drop(client.open_painted("busy", "org.deskrinch.Busy", paint(), None)?);
    }
    // Mapped only once every request before it has been handled.
    let last = client.open_painted("busy", "org.deskrinch.Busy", paint(), None)?;
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::fs::write(&args.done, format!("{at}\n"))?;
    drop(last);
    if Instant::now() > limit {
        return Err(BusyError::NeverFinished {
            rounds: args.rounds,
            done: args.rounds,
            within: args.within,
        });
    }
    Ok(())
}

//! A deadline on one property case, kept by a thread of its own: a case
//! that runs past [`CASE_LIMIT`] ends the test binary, with a sentence
//! saying which property, how long, and where its input was saved.
//!
//! A decode that spins cannot be stopped from inside its own thread, and
//! proptest's own timeout (a forked child per run, killed by its parent)
//! reported a hang only after shrinking it, a limit per shrinking step,
//! which took hours. So proptest runs its cases in process with no timeout,
//! shrinking ordinary failures as it always does, and this watchdog reports
//! a hang at once, with the exact input it was given, unshrunk.
//!
//! The watchdog is a scoped thread that borrows the case's input: it writes
//! the input out only when it fires, so a case that finishes in time pays
//! for no copy and no text, only the thread (about 23 µs a case in a debug
//! build, 2026-11-23, against 14 ms for the slowest honest case of
//! deskrinch-picture's properties). Taken before each case instead, the
//! `Debug` text of notification-service's image input cost 662 µs at its
//! largest, for a case of under a microsecond.
//!
//! The sentence is written to the process's standard error directly, not
//! with `eprintln!`. libtest captures what a test prints, a thread the test
//! spawns inherits the capture, and the abort throws the capture away: with
//! `eprintln!` a hang ended the binary with no word of why. The abort tests
//! run their child as an ordinary run does, captured, to hold this.

use std::fmt::Debug;
use std::io::Write as _;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// How long one case may run.
///
/// It is set far above an honest case, because a false alarm from load is a
/// flake (reruns, and lost trust, every time), while a hang reported a
/// minute late costs a minute once. Measured on 2026-11-23: the slowest case
/// of deskrinch-picture's properties took 14 ms of CPU in a debug build; at
/// loads of 40 to 60 a case's wall time was 4 to 5 times its CPU time (68 ms
/// at load 42), so that case would take about 0.3 s at a load of 190 and
/// about 0.8 s at 500: 60 s is about 200 and 75 times those.
pub const CASE_LIMIT: Duration = Duration::from_secs(60);

/// Runs `case` under [`CASE_LIMIT`]. If it runs past it, `input` (the bytes
/// the case decodes) is saved: given to the code under test again, the file
/// replays the case.
pub fn case_bytes<T>(property: &str, input: &[u8], case: impl FnOnce() -> T) -> T {
    within(
        property,
        &Input::Bytes(input),
        CASE_LIMIT,
        &std::env::temp_dir(),
        case,
    )
}

/// Runs `case` under [`CASE_LIMIT`]. If it runs past it, `input`'s `Debug`
/// text is saved: a structured input is not replayable from a file, but its
/// text is enough to rebuild the case by hand.
pub fn case_debug<T>(property: &str, input: &(impl Debug + Sync), case: impl FnOnce() -> T) -> T {
    within(
        property,
        &Input::Text(input),
        CASE_LIMIT,
        &std::env::temp_dir(),
        case,
    )
}

/// What a hung case is reported with, borrowed from the case.
pub(crate) enum Input<'a> {
    /// The bytes the case decodes, replayable.
    Bytes(&'a [u8]),
    /// A structured input, written out as its `Debug` text.
    Text(&'a (dyn Debug + Sync)),
}

/// Runs `case` with a watchdog that, if `limit` passes first, saves `input`
/// in `dir`, says so, and aborts the process. The watchdog is a scoped
/// thread: when the case returns or unwinds, its channel closes, the
/// watchdog returns, and the scope joins it, so no thread outlives the case
/// and a panic goes on to proptest as before.
pub(crate) fn within<T>(
    property: &str,
    input: &Input<'_>,
    limit: Duration,
    dir: &Path,
    case: impl FnOnce() -> T,
) -> T {
    let (stand_down, wait) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("fuzz watchdog".to_owned())
            .spawn_scoped(scope, move || match wait.recv_timeout(limit) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
                Err(RecvTimeoutError::Timeout) => report_and_abort(property, input, limit, dir),
            })
            .unwrap_or_else(|error| panic!("the fuzz watchdog's thread could not start: {error}"));
        let result = case();
        // Closing the channel wakes the watchdog, which returns; on a panic
        // the unwinding closes it the same way.
        drop(stand_down);
        result
    })
}

/// Saves `input` in `dir`, says what happened in one sentence on standard
/// error, and aborts the process: a case that has run past its limit is
/// still running on its own thread, and nothing else can stop it.
fn report_and_abort(property: &str, input: &Input<'_>, limit: Duration, dir: &Path) -> ! {
    let name: String = property
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let (extension, bytes, kind) = match input {
        Input::Bytes(bytes) => (
            "bin",
            bytes.to_vec(),
            "the bytes it was decoding; give them to the code under test again to replay it",
        ),
        Input::Text(value) => (
            "txt",
            format!("{value:#?}").into_bytes(),
            "its input's Debug text, to rebuild the case by hand",
        ),
    };
    let path = dir.join(format!(
        "deskrinch-fuzz-hang-{name}-{}.{extension}",
        std::process::id()
    ));
    let saved = match std::fs::write(&path, bytes) {
        Ok(()) => format!("in {}: {kind}", path.display()),
        Err(error) => format!(
            "not saved ({} could not be written: {error})",
            path.display()
        ),
    };
    // Written to the process's standard error itself, not through
    // `eprintln!`: libtest captures what a test prints, a thread a test
    // spawns inherits that capture, and the abort would discard it.
    let sentence = format!(
        "fuzz watchdog: a case of {property} ran past its {} s limit and the test binary is \
         aborted; its input is {saved}\n",
        limit.as_secs()
    );
    let mut stderr = std::io::stderr().lock();
    // Nothing is left to do if standard error cannot be written: the abort
    // still ends the hang.
    let _written = stderr
        .write_all(sentence.as_bytes())
        .and_then(|()| stderr.flush());
    std::process::abort()
}

#[cfg(test)]
#[path = "watchdog_tests.rs"]
mod tests;

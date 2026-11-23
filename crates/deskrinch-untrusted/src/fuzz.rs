//! What the property tests of every untrusted-input parser share, in this
//! crate and in the crates that use it: how many cases, and the deadline on
//! each case. Only with the `proptest` feature, which those crates turn on
//! in their dev-dependencies.
//!
//! Every property runs each case through [`case_bytes`] (an input that is
//! bytes) or [`case_debug`] (a structured input), whose watchdog ends the
//! test binary when a case runs past [`CASE_LIMIT`]: a hang is reported at
//! once, in one sentence, with the case's input saved in the temporary
//! directory. proptest itself runs the cases in process with no timeout, so
//! an ordinary failure (a panic, a bound passed) shrinks as proptest
//! shrinks it and prints its minimal case.
//!
//! A hang is reported unshrunk. To shrink one by hand: for bytes, cut the
//! saved file down (halves, then single bytes) while the code under test
//! still hangs on it; for a `Debug` text, rebuild the case from it in a
//! test of its own and cut it down the same way.

use proptest::test_runner::Config;

mod watchdog;

pub use watchdog::{CASE_LIMIT, case_bytes, case_debug};

/// 512 cases a property (set here: `PROPTEST_CASES` does not change it),
/// run in process with no timeout and shrunk as proptest shrinks by
/// default; each case's deadline is the watchdog's ([`case_bytes`],
/// [`case_debug`]).
#[must_use]
pub fn config() -> Config {
    Config {
        cases: 512,
        ..Config::default()
    }
}

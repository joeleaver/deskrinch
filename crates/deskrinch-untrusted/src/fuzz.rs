//! The configuration the property tests of every untrusted-input parser
//! share, in this crate and in the crates that use it: how many cases, and
//! how long one case may take. Only with the `proptest` feature, which
//! those crates turn on in their dev-dependencies.

use proptest::test_runner::Config;

/// 512 cases a property (`PROPTEST_CASES` overrides it). Each case runs in
/// a forked child with a 10 s timeout, so a case that hangs or runs away
/// fails the test instead of stalling CI.
#[must_use]
pub fn config() -> Config {
    Config {
        cases: 512,
        timeout: 10_000,
        ..Config::default()
    }
}

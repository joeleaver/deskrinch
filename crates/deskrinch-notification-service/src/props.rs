//! The configuration the property tests of the untrusted-input parsers
//! share: how many cases, and how long one case may take.

use proptest::test_runner::Config;

/// 512 cases a property (`PROPTEST_CASES` overrides it). Each case runs in
/// a forked child with a 10 s timeout, so a case that hangs or runs away
/// fails the test instead of stalling CI.
pub(crate) fn config() -> Config {
    Config {
        cases: 512,
        timeout: 10_000,
        ..Config::default()
    }
}

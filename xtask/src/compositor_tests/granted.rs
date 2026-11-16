//! Binaries built with a suite's tests, besides its test binary, that the
//! runner grants under KWin what the suite's grant says: a test that runs
//! a binary of its package (`CARGO_BIN_EXE_*`) as a client of the session,
//! which KWin grants by its own executable, not the test's.

use super::build::Target;
use super::suites::INPUT_CLIENT;

/// Each suite's target, and the `[[bin]]` of its package granted with it:
/// xtask's own binary, which `input_client` runs as measure's input client.
const GRANTED_BINARIES: &[(Target, &str)] = &[(INPUT_CLIENT, "xtask")];

/// The binaries granted with `target`'s suite.
pub(super) fn with(target: Target) -> impl Iterator<Item = &'static str> {
    GRANTED_BINARIES
        .iter()
        .filter(move |(granted, _)| *granted == target)
        .map(|&(_, binary)| binary)
}

/// The desktop id under which `binary` is granted with a suite whose grant
/// is `id`.
pub(super) fn id(id: &str, binary: &str) -> String {
    format!("{id}.{binary}")
}

#[cfg(test)]
mod tests {
    use super::super::build::Backend;
    use super::*;

    #[test]
    fn the_input_client_suite_grants_xtask_and_no_other_suite_grants_a_binary() {
        assert_eq!(with(INPUT_CLIENT).collect::<Vec<_>>(), ["xtask"]);
        let other = Target {
            test: "desktop",
            ..INPUT_CLIENT
        };
        assert_eq!(with(other).count(), 0);
        let on_libwayland = Target {
            backend: Backend::System,
            ..INPUT_CLIENT
        };
        assert_eq!(with(on_libwayland).count(), 0);
        assert_eq!(
            id("org.deskrinch.InputClientTests", "xtask"),
            "org.deskrinch.InputClientTests.xtask"
        );
    }
}

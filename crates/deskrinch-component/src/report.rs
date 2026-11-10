//! Reporting, through `tracing`.
//!
//! A component logs as rinch does, so its messages and rinch's share one
//! filter and one format: `RUST_LOG` picks what is shown (`warn` and worse
//! when it is unset, the level rinch's window starts its own logging
//! with), and events go to standard error, which the session's journal
//! keeps.
//!
//! [`init`] must come first in `main`: a component reads its configuration
//! before its window runs, and an event reported before a subscriber is
//! installed is lost.

use std::error::Error;

use deskrinch_config::{Diagnostic, Severity};

/// What is shown when `RUST_LOG` is not set: rinch's own default.
const DEFAULT_FILTER: &str = "warn";

/// Installs the process's `tracing` subscriber, through rinch's own
/// initialiser ([`rinch::init_logging_with_filter`]), so that rinch's
/// window finds it installed and keeps it. Idempotent.
pub fn init() {
    rinch::init_logging_with_filter(DEFAULT_FILTER);
}

/// Reports that `what` failed, with `error`'s chain of causes.
pub fn error(what: &str, error: &dyn Error) {
    tracing::error!("{what}: {}", chain(error));
}

/// Reports a configuration diagnostic (`path:line:column: severity: ...`)
/// at its severity: an error, or a warning.
pub fn diagnostic(diagnostic: &Diagnostic) {
    match diagnostic.severity() {
        Severity::Error => tracing::error!("{diagnostic}"),
        Severity::Warning => tracing::warn!("{diagnostic}"),
    }
}

/// `error` and its causes, joined by `: `.
fn chain(error: &dyn Error) -> String {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(inner) = cause {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        cause = inner.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    #[derive(Debug)]
    struct Layer(&'static str, Option<Box<Layer>>);

    impl fmt::Display for Layer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl Error for Layer {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            match &self.1 {
                Some(inner) => Some(&**inner),
                None => None,
            }
        }
    }

    #[test]
    fn writes_the_whole_chain_of_causes() {
        let error = Layer(
            "cannot start the clock",
            Some(Box::new(Layer(
                "the real-time timer failed",
                Some(Box::new(Layer("permission denied", None))),
            ))),
        );
        assert_eq!(
            chain(&error),
            "cannot start the clock: the real-time timer failed: permission denied"
        );
        assert_eq!(chain(&Layer("alone", None)), "alone");
    }
}

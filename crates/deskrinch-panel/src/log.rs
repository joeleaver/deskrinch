//! Reporting to standard error, which the session's journal keeps.

use std::error::Error;
use std::fmt::Display;

/// Writes `error` and its causes, after what failed.
pub(crate) fn error(what: &str, error: &dyn Error) {
    let mut message = format!("deskrinch-panel: {what}: {error}");
    let mut cause = error.source();
    while let Some(inner) = cause {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        cause = inner.source();
    }
    eprintln!("{message}");
}

/// Writes a configuration diagnostic, already `path:line:column: ...`.
pub(crate) fn diagnostic(diagnostic: &impl Display) {
    eprintln!("deskrinch-panel: {diagnostic}");
}

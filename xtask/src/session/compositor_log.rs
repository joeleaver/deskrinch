//! A compositor's output as errors and messages quote it: said to be empty
//! when it is, so that "its log:" is never followed by nothing.

use std::fmt;

/// What a session's compositor has printed so far ([`super::Session::log`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CompositorLog(String);

/// What an empty log is printed as.
const EMPTY: &str = "(empty: the compositor had printed nothing)";

impl CompositorLog {
    /// The log `text`.
    pub(crate) fn new(text: String) -> Self {
        Self(text)
    }

    /// Its text, for reading what the compositor said.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CompositorLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.trim().is_empty() {
            f.write_str(EMPTY)
        } else {
            f.write_str(&self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_log_is_said_to_be_empty() {
        assert_eq!(CompositorLog::default().to_string(), EMPTY);
        assert_eq!(CompositorLog::new(" \n".into()).to_string(), EMPTY);
        assert_eq!(
            CompositorLog::new("00:00:00.001 [sway] started\n".into()).to_string(),
            "00:00:00.001 [sway] started\n"
        );
    }
}

//! Whether this process runs on a CI runner, read from GitHub Actions' own
//! variable, [`CI_VARIABLE`].
//!
//! Two questions are asked of it, and they want different strictness:
//!
//! - a permit (installing system packages, `sudo` in a test, a unit in the
//!   user's systemd) asks whether this is certainly a runner
//!   ([`Ci::certainly_a_runner`]): only `GITHUB_ACTIONS=true`, so that an
//!   unclear value does not open it, and refuses otherwise with
//!   [`NotCi`] ([`Ci::only_on_a_runner`]);
//! - a refusal (the owner's interactive path of `cargo xtask desktop`,
//!   which CI must not take) asks whether this may be a runner
//!   ([`Ci::maybe_a_runner`]): the variable set at all, so that an unclear
//!   value refuses.
//!
//! Callers ask by those names; [`Ci`]'s variants are for saying what the
//! variable held.

use std::ffi::OsStr;
use std::fmt;

/// The variable GitHub Actions sets to `true` in every job.
pub const CI_VARIABLE: &str = "GITHUB_ACTIONS";

/// What [`CI_VARIABLE`] held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ci {
    /// `true`: a runner.
    True,
    /// Set, to something other than `true`.
    Other {
        /// The value, lossily as text.
        value: String,
    },
    /// Not set.
    Unset,
}

impl Ci {
    /// What `value`, [`CI_VARIABLE`]'s value or `None` when it is not set,
    /// says.
    #[must_use]
    pub fn from_value(value: Option<&OsStr>) -> Self {
        match value {
            None => Self::Unset,
            Some(value) if value == "true" => Self::True,
            Some(value) => Self::Other {
                value: value.to_string_lossy().into_owned(),
            },
        }
    }

    /// What this process's [`CI_VARIABLE`] says.
    #[must_use]
    pub fn read() -> Self {
        Self::from_value(std::env::var_os(CI_VARIABLE).as_deref())
    }

    /// Whether this is certainly a CI runner: the variable is exactly
    /// `true`. What a permit asks.
    #[must_use]
    pub fn certainly_a_runner(&self) -> bool {
        matches!(self, Self::True)
    }

    /// Whether this may be a CI runner: the variable is set at all. What a
    /// refusal asks.
    #[must_use]
    pub fn maybe_a_runner(&self) -> bool {
        !matches!(self, Self::Unset)
    }

    /// A permit's answer: `Ok` only when this is certainly a runner.
    ///
    /// # Errors
    ///
    /// [`NotCi`], saying what the variable held.
    pub fn only_on_a_runner(&self) -> Result<(), NotCi> {
        if self.certainly_a_runner() {
            Ok(())
        } else {
            Err(NotCi { held: self.clone() })
        }
    }
}

impl fmt::Display for Ci {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::True => write!(f, "{CI_VARIABLE} is \"true\""),
            Self::Other { value } => write!(f, "{CI_VARIABLE} is {value:?}"),
            Self::Unset => write!(f, "{CI_VARIABLE} is not set"),
        }
    }
}

/// Why a permit refused: this is not certainly a CI runner. Its text
/// follows the name of what refused ("ci-packages", "the systemd scope
/// test").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotCi {
    /// What the variable held.
    pub held: Ci,
}

impl fmt::Display for NotCi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "runs only on a CI runner ({CI_VARIABLE}=true), and here {}",
            self.held
        )
    }
}

impl std::error::Error for NotCi {}

#[cfg(test)]
mod tests {
    use super::*;

    fn ci(value: Option<&str>) -> Ci {
        Ci::from_value(value.map(OsStr::new))
    }

    #[test]
    fn only_true_is_certainly_a_runner() {
        assert!(ci(Some("true")).certainly_a_runner());
        for value in [
            None,
            Some(""),
            Some("false"),
            Some("1"),
            Some("TRUE"),
            Some("true "),
        ] {
            assert!(!ci(value).certainly_a_runner(), "{value:?}");
        }
    }

    #[test]
    fn any_value_may_be_a_runner() {
        for value in ["true", "", "false", "1", "TRUE"] {
            assert!(ci(Some(value)).maybe_a_runner(), "{value:?}");
        }
        assert!(!ci(None).maybe_a_runner());
    }

    #[test]
    fn a_permit_refuses_saying_what_the_variable_held() {
        assert_eq!(ci(Some("true")).only_on_a_runner(), Ok(()));
        let unset = ci(None).only_on_a_runner().unwrap_err();
        assert_eq!(
            unset.to_string(),
            "runs only on a CI runner (GITHUB_ACTIONS=true), and here GITHUB_ACTIONS is not set"
        );
        let unclear = ci(Some("TRUE")).only_on_a_runner().unwrap_err();
        assert_eq!(
            unclear.held,
            Ci::Other {
                value: "TRUE".to_owned()
            }
        );
        assert!(
            unclear
                .to_string()
                .ends_with("and here GITHUB_ACTIONS is \"TRUE\""),
            "{unclear}"
        );
    }
}

//! The programs a suite's tests start that are not of their own crate
//! (the panel, for the wallpaper's layering test). A suite declares them
//! (its `programs`, in `xtask/src/compositor_tests/suites.rs`), and
//! `cargo xtask compositor-tests` builds them with its tests and hands each
//! one's path to them through the environment ([`environment`]); a test
//! asks for one by its package ([`program`]). Nothing looks beside a test's
//! own binary for another package's: a selected run builds only what the
//! chosen suites declare, and anything found there could be from any
//! earlier build.
//!
//! The variables' names are made here only, for both sides.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

/// The variable listing the packages the suite declared, `,` between.
const DECLARED: &str = "DESKRINCH_TEST_PROGRAMS";

/// The variable holding `package`'s program's path.
fn variable(package: &str) -> String {
    let name: String = package
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("DESKRINCH_TEST_PROGRAM_{name}")
}

/// The environment a suite's tests run with for its `programs`, each a
/// package and its built program's path: every program's variable, and the
/// list of what was declared.
#[must_use]
pub fn environment<'a>(
    programs: impl IntoIterator<Item = (&'a str, &'a Path)>,
) -> Vec<(String, OsString)> {
    let mut declared = Vec::new();
    let mut env = Vec::new();
    for (package, path) in programs {
        declared.push(package);
        env.push((variable(package), path.as_os_str().to_owned()));
    }
    env.push((DECLARED.to_owned(), declared.join(",").into()));
    env
}

/// The path of `package`'s program, as the run built it for this suite.
///
/// # Errors
///
/// [`UndeclaredProgram`] when the suite did not declare it, naming what it
/// did declare.
pub fn program(package: &str) -> Result<PathBuf, UndeclaredProgram> {
    program_in(package, |name| std::env::var_os(name))
}

/// [`program`], reading variables through `var`.
fn program_in(
    package: &str,
    var: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf, UndeclaredProgram> {
    var(&variable(package))
        .map(PathBuf::from)
        .ok_or_else(|| UndeclaredProgram {
            package: package.to_owned(),
            declared: var(DECLARED).as_deref().map(declared).unwrap_or_default(),
        })
}

/// The packages in the list of declared programs.
fn declared(list: &OsStr) -> Vec<String> {
    list.to_string_lossy()
        .split(',')
        .filter(|package| !package.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A test asked for a program its suite did not declare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndeclaredProgram {
    /// The package asked for.
    pub package: String,
    /// What the suite declared.
    pub declared: Vec<String>,
}

impl fmt::Display for UndeclaredProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let declared = if self.declared.is_empty() {
            "none".to_owned()
        } else {
            self.declared.join(", ")
        };
        write!(
            f,
            "{} is not one of this suite's programs (it declares {declared}): add it to the \
             suite's `programs` in xtask/src/compositor_tests/suites.rs, which `cargo xtask \
             compositor-tests` builds and hands to the tests",
            self.package
        )
    }
}

impl std::error::Error for UndeclaredProgram {}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn lookup(env: &[(String, OsString)]) -> impl Fn(&str) -> Option<OsString> + '_ {
        let map: HashMap<&str, &OsString> = env.iter().map(|(k, v)| (k.as_str(), v)).collect();
        move |name| map.get(name).map(|v| (*v).clone())
    }

    #[test]
    fn a_declared_program_is_found_by_its_package() {
        let env = environment([(
            "deskrinch-panel",
            Path::new("/target/debug/deskrinch-panel"),
        )]);
        assert_eq!(
            program_in("deskrinch-panel", lookup(&env)),
            Ok(PathBuf::from("/target/debug/deskrinch-panel"))
        );
        assert!(
            env.iter()
                .any(|(name, _)| name == "DESKRINCH_TEST_PROGRAM_DESKRINCH_PANEL")
        );
    }

    #[test]
    fn an_undeclared_program_says_what_the_suite_declares() {
        let env = environment([("deskrinch-panel", Path::new("/p"))]);
        let error = program_in("deskrinch-launcher", lookup(&env)).unwrap_err();
        assert_eq!(error.declared, ["deskrinch-panel"]);
        let said = error.to_string();
        assert!(
            said.contains("deskrinch-launcher is not one of this suite's programs"),
            "{said}"
        );
        assert!(said.contains("it declares deskrinch-panel"), "{said}");
        assert!(said.contains("`programs`"), "{said}");
        let none = program_in("deskrinch-panel", lookup(&environment([]))).unwrap_err();
        assert!(none.to_string().contains("it declares none"), "{none}");
    }
}

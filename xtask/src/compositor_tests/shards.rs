//! The compositor tests split into shards for CI: each shard is one
//! compositor's share of the suites' rows, run by a job of its own from one
//! build ([`super::bundle`]), and one shard per compositor also runs the
//! session and measure tests ([`crate::session_tests`]).
//!
//! A shard names its rows by their packages, or takes the rest of its
//! compositor's rows; so every row under a compositor is in exactly one of
//! its shards, which the tests below check over the whole table. A
//! compositor gains a shard here when its suites grow: the workflow takes
//! its matrix from [`matrix`], and names none itself.

use serde::Serialize;

use super::suites::SUITES;
use crate::session::Compositor;

/// Which of its compositor's rows a shard runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rows {
    /// The rows whose test binaries are these packages'.
    Packages(&'static [&'static str]),
    /// Every row of the compositor that no other of its shards names.
    Rest,
}

/// One shard: a compositor's share of the suites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shard {
    /// Its name, as `--shard` takes it and CI's job names it.
    pub(crate) name: &'static str,
    /// The compositor it runs under.
    pub(crate) compositor: Compositor,
    rows: Rows,
    /// Whether this shard also runs the session and measure tests, and the
    /// panel's measure under its compositor: one shard per compositor.
    pub(crate) session_tests: bool,
}

/// Every shard. KWin's notification suite (about 340 s on CI) goes beside
/// the session and measure tests (about 180 s), and the rest of KWin's
/// suites (about 550 s, the wallpaper's four rows about 110 s of it) in a
/// shard of their own; sway's and labwc's suites and tests fit one shard
/// each (about 11 minutes). The wallpaper's rows stay with the rest: the
/// two KWin jobs took 9.5 and 9.4 minutes on CI (run 45311180710), and
/// beside the notifications the rows would leave them about 220 s apart.
pub(crate) const SHARDS: &[Shard] = &[
    Shard {
        name: "kwin-notifications",
        compositor: Compositor::Kwin,
        rows: Rows::Packages(&["deskrinch-notifications"]),
        session_tests: true,
    },
    Shard {
        name: "kwin-others",
        compositor: Compositor::Kwin,
        rows: Rows::Rest,
        session_tests: false,
    },
    Shard {
        name: "sway",
        compositor: Compositor::Sway,
        rows: Rows::Rest,
        session_tests: true,
    },
    Shard {
        name: "labwc",
        compositor: Compositor::Labwc,
        rows: Rows::Rest,
        session_tests: true,
    },
];

/// Why a shard cannot be chosen.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ShardError {
    /// `--shard` named none of [`SHARDS`].
    #[error("no shard is named {name:?}; the shards are: {}", names().join(", "))]
    Unknown {
        /// What was asked for.
        name: String,
    },
}

/// The shards' names, in order.
fn names() -> Vec<&'static str> {
    SHARDS.iter().map(|shard| shard.name).collect()
}

impl Shard {
    /// The shard named `name`.
    pub(crate) fn named(name: &str) -> Result<&'static Self, ShardError> {
        SHARDS
            .iter()
            .find(|shard| shard.name == name)
            .ok_or_else(|| ShardError::Unknown {
                name: name.to_owned(),
            })
    }

    /// Its rows in [`SUITES`], in the table's order.
    pub(crate) fn rows(&self) -> Vec<usize> {
        (0..SUITES.len())
            .filter(|&row| self.takes(SUITES[row].target.package, SUITES[row].compositors))
            .collect()
    }

    /// Whether it runs a row of `package`'s under `compositors`.
    fn takes(&self, package: &str, compositors: &[Compositor]) -> bool {
        if !compositors.contains(&self.compositor) {
            return false;
        }
        match self.rows {
            Rows::Packages(packages) => packages.contains(&package),
            Rows::Rest => !SHARDS.iter().any(|other| {
                other.compositor == self.compositor
                    && matches!(other.rows, Rows::Packages(named) if named.contains(&package))
            }),
        }
    }
}

/// One entry of CI's matrix: what a shard job is given.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct Entry {
    shard: &'static str,
    compositor: &'static str,
    session_tests: bool,
}

/// CI's matrix of shard jobs, as `strategy.matrix` takes it from JSON:
/// `{"include": [{"shard", "compositor", "session_tests"}, ...]}`.
pub(crate) fn matrix() -> String {
    #[derive(Serialize)]
    struct Matrix {
        include: Vec<Entry>,
    }
    let include = SHARDS
        .iter()
        .map(|shard| Entry {
            shard: shard.name,
            compositor: shard.compositor.name(),
            session_tests: shard.session_tests,
        })
        .collect();
    serde_json::to_string(&Matrix { include }).unwrap_or_default()
}

/// `cargo xtask compositor-shards`: prints [`matrix`].
pub(crate) fn run() -> crate::Outcome {
    println!("{}", matrix());
    crate::Outcome::Passed
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPOSITORS: [Compositor; 3] = [Compositor::Kwin, Compositor::Sway, Compositor::Labwc];

    #[test]
    fn every_row_under_a_compositor_is_in_exactly_one_of_its_shards() {
        for (row, suite) in SUITES.iter().enumerate() {
            for compositor in COMPOSITORS {
                let holding: Vec<_> = SHARDS
                    .iter()
                    .filter(|shard| shard.compositor == compositor && shard.rows().contains(&row))
                    .map(|shard| shard.name)
                    .collect();
                let expected = usize::from(suite.compositors.contains(&compositor));
                assert_eq!(
                    holding.len(),
                    expected,
                    "{} under {}: {holding:?}",
                    suite.target.name(),
                    compositor.name()
                );
            }
        }
    }

    #[test]
    fn each_compositor_runs_the_session_tests_in_one_shard() {
        for compositor in COMPOSITORS {
            let running = SHARDS
                .iter()
                .filter(|shard| shard.compositor == compositor && shard.session_tests)
                .count();
            assert_eq!(running, 1, "{}", compositor.name());
        }
    }

    #[test]
    fn every_package_a_shard_names_has_a_row_under_its_compositor() {
        for shard in SHARDS {
            if let Rows::Packages(packages) = shard.rows {
                for package in packages {
                    assert!(
                        SUITES.iter().any(|suite| suite.target.package == *package
                            && suite.compositors.contains(&shard.compositor)),
                        "{}: {package}",
                        shard.name
                    );
                }
            }
            assert!(!shard.rows().is_empty(), "{} runs nothing", shard.name);
        }
    }

    #[test]
    fn a_shard_keeps_the_table_s_order_and_the_session_end_last() {
        let others = Shard::named("kwin-others").unwrap().rows();
        assert!(others.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(others.last(), Some(&(SUITES.len() - 1)));
    }

    #[test]
    fn names_are_unique_and_unknown_ones_are_listed() {
        let mut seen = names();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), SHARDS.len());
        let message = Shard::named("kwin").unwrap_err().to_string();
        assert!(
            message.contains("kwin-notifications, kwin-others"),
            "{message}"
        );
    }

    #[test]
    fn the_matrix_lists_every_shard() {
        let matrix: serde_json::Value = serde_json::from_str(&matrix()).unwrap();
        let include = matrix["include"].as_array().unwrap();
        assert_eq!(include.len(), SHARDS.len());
        assert_eq!(include[0]["shard"], "kwin-notifications");
        assert_eq!(include[0]["compositor"], "kwin");
        assert_eq!(include[0]["session_tests"], true);
    }
}

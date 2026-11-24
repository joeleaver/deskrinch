//! What a run's suites run: built by the run itself, or taken from the
//! bundle CI built once ([`super::bundle`]), and the grants KWin is given
//! for them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::build::{self, Target};
use super::bundle::Bundle;
use super::granted;
use super::run_dir::RunDir;
use super::selection::Selection;
use super::suites::{Grant, SUITES};
use crate::components::{self, Profile};
use crate::error::Error;

/// What a run's suites run: the chosen suites' test binaries, by their
/// rows in [`SUITES`], and, when they declare programs
/// ([`needs_components`]), every component, by package. A run that built
/// them keeps the components in its own directory for as long as it lasts;
/// a bundle's are the job's own already.
pub(super) struct Built {
    pub(super) tests: BTreeMap<usize, PathBuf>,
    /// By row, the binaries granted with its suite ([`granted`]).
    pub(super) granted: BTreeMap<usize, Vec<(&'static str, PathBuf)>>,
    pub(super) programs: BTreeMap<&'static str, PathBuf>,
    _kept: Option<RunDir>,
}

/// What KWin grants the binary of `row`'s suite: its own grant, or, for a
/// binary run by more than one row, the one row that declares it
/// (`suites::tests::each_binary_is_granted_at_most_once`), so that a run
/// that chooses only another of its rows grants it too.
fn grant_of(row: usize) -> Option<&'static Grant> {
    let target = &SUITES[row].target;
    SUITES
        .iter()
        .filter(|suite| suite.target == *target)
        .find_map(|suite| suite.grant.as_ref())
}

/// Whether the suites of `rows` declare a program, and so whether the run
/// needs the components.
fn needs_components(rows: &[usize]) -> bool {
    rows.iter().any(|&row| !SUITES[row].programs.is_empty())
}

impl Built {
    /// Builds the chosen suites' test binaries, then, when they declare
    /// programs, every component ([`crate::components::build`]), kept in the
    /// run's own directory.
    pub(super) fn build(root: &Path, selection: &Selection) -> Result<Self, Error> {
        let mut targets: Vec<Target> = Vec::new();
        for &row in selection.rows() {
            if !targets.contains(&SUITES[row].target) {
                targets.push(SUITES[row].target);
            }
        }
        let built = build::test_binaries(root, &targets)?;
        let of_row = |row: usize| {
            let at = targets
                .iter()
                .position(|target| *target == SUITES[row].target);
            &built[at.unwrap_or_default()]
        };
        let rows = selection.rows();
        let tests = rows
            .iter()
            .map(|&row| (row, of_row(row).test.clone()))
            .collect();
        let granted = rows
            .iter()
            .map(|&row| (row, of_row(row).granted.clone()))
            .collect();
        if !needs_components(rows) {
            return Ok(Self {
                tests,
                granted,
                programs: BTreeMap::new(),
                _kept: None,
            });
        }
        let components = components::build(root, Profile::Dev)?;
        let keeping = |source| Error::Io {
            what: "keep the components for the run".into(),
            source,
        };
        let kept = RunDir::in_target(&components.target).map_err(keeping)?;
        let programs = components
            .built
            .binaries()
            .map(|(package, binary)| Ok((package, kept.keep(package, binary).map_err(keeping)?)))
            .collect::<Result<_, Error>>()?;
        Ok(Self {
            tests,
            granted,
            programs,
            _kept: Some(kept),
        })
    }

    /// The chosen suites' binaries in `bundle`, built by nobody here.
    pub(super) fn from_bundle(bundle: &Bundle, selection: &Selection) -> Result<Self, Error> {
        let rows = selection.rows();
        let mut tests = BTreeMap::new();
        let mut granted = BTreeMap::new();
        for &row in rows {
            let target = SUITES[row].target;
            tests.insert(row, bundle.test(target)?);
            granted.insert(row, bundle.granted(target)?);
        }
        let programs = if needs_components(rows) {
            bundle.components()?
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            tests,
            granted,
            programs,
            _kept: None,
        })
    }

    /// Installs, for each chosen suite whose binary KWin grants something
    /// ([`grant_of`]), the one desktop file that grants it all of it, once
    /// per binary; and for each program they declare, what the shell grants
    /// that component ([`crate::grant`]).
    pub(super) fn grant_all(&self, data_home: &Path) -> Result<(), Error> {
        let mut granted_targets = Vec::new();
        for (&row, binary) in &self.tests {
            let Some(grant) = grant_of(row) else {
                continue;
            };
            if granted_targets.contains(&&SUITES[row].target) {
                continue;
            }
            granted_targets.push(&SUITES[row].target);
            let entry =
                crate::grant::client(grant.id, grant.name, binary, grant.wayland, grant.dbus)?;
            crate::grant::install_entry(&entry, data_home)?;
            for (bin, path) in self.granted.get(&row).into_iter().flatten() {
                let id = granted::id(grant.id, bin);
                let entry = crate::grant::client(&id, grant.name, path, grant.wayland, grant.dbus)?;
                crate::grant::install_entry(&entry, data_home)?;
            }
        }
        for &row in self.tests.keys() {
            for program in SUITES[row].programs {
                crate::grant::install(program, &self.programs[program.package], data_home)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The row of the suite named `name`.
    fn row(name: &str) -> usize {
        SUITES
            .iter()
            .position(|suite| suite.target.name() == name)
            .unwrap_or_else(|| panic!("no suite named {name}"))
    }

    /// Every row of a binary is granted what its one declaring row grants:
    /// the wallpaper's row on the pass with outputs of its own declares no
    /// grant, and a run of that row alone still grants the binary.
    #[test]
    fn every_row_of_a_granted_binary_is_granted() {
        let rows: Vec<usize> = SUITES
            .iter()
            .enumerate()
            .filter(|(_, suite)| suite.target.name() == "deskrinch-wallpaper compositor")
            .map(|(row, _)| row)
            .collect();
        assert_eq!(rows.len(), 2, "control: the wallpaper has two rows");
        assert!(
            rows.iter().any(|&row| SUITES[row].grant.is_none()),
            "control"
        );
        for row in rows {
            let grant = grant_of(row).expect("granted");
            assert_eq!(grant.id, "org.deskrinch.WallpaperTests");
        }
    }

    #[test]
    fn components_are_needed_only_for_a_run_that_starts_one() {
        let panel = row("deskrinch-panel compositor");
        let screen = row("deskrinch-test-screen compositor");
        assert!(needs_components(&[panel]));
        assert!(needs_components(&[screen, panel]));
        assert!(!needs_components(&[screen]));
        assert!(!needs_components(&[]));
        let all: Vec<usize> = (0..SUITES.len()).collect();
        assert!(needs_components(&all));
    }
}

//! The sessions the suites run in, by how their outputs are: each pass is
//! one isolated session per compositor (and per renderer, for the suites
//! that draw), and a suite runs in the passes its row names
//! ([`super::suites`]).

use deskrinch_test_wayland::Transform;

use crate::session::{OutputSize, Outputs, Scale};

/// How many outputs the sessions have, side by side: the window-management
/// tests move windows between two, and the screen tests tell them apart.
const OUTPUTS: u32 = 2;

/// A session the suites run in, by how its outputs are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pass {
    /// Scale 1, upright: every suite.
    Plain,
    /// Scale 2, for the suites that check scaling.
    Scaled,
    /// Turned a quarter, for the suites that check transforms.
    Rotated,
    /// Mirrored and turned a quarter (`flipped-90`): a flip as well as a
    /// swap of the axes, for the suites that check transforms.
    Flipped,
    /// A 3840x2160 output beside a 1920x1080 one, at scale 1, upright
    /// ([`UHD_BESIDE_FULL_HD`]): outputs of their own, for the tests that
    /// need a wide output, or one output larger than the other. Its rows
    /// run only the tests written for it, which check that they are on
    /// it (`deskrinch_test_client::require_outputs`). Each such pass is
    /// two more sessions per compositor in a run (software and GPU), so a
    /// second one is added only for outputs this one cannot give.
    UhdBesideFullHd,
}

/// The outputs of [`Pass::UhdBesideFullHd`].
pub(super) const UHD_BESIDE_FULL_HD: [OutputSize; 2] = [
    OutputSize {
        width: 3840,
        height: 2160,
    },
    OutputSize {
        width: 1920,
        height: 1080,
    },
];

impl Pass {
    /// The passes of the default outputs, in the order they run: what a
    /// suite that checks scaling and transforms runs in.
    pub(super) const ALL: [Self; 4] = [Self::Plain, Self::Scaled, Self::Rotated, Self::Flipped];

    /// Every pass, those with outputs of their own last, in the order they
    /// run.
    pub(super) const EVERY: [Self; 5] = [
        Self::Plain,
        Self::Scaled,
        Self::Rotated,
        Self::Flipped,
        Self::UhdBesideFullHd,
    ];

    /// The session's outputs in this pass.
    pub(super) fn outputs(self) -> Outputs {
        let normal = Transform::Normal.into();
        match self {
            Self::Plain => Outputs::plain(OUTPUTS),
            Self::Scaled => Outputs::new(OUTPUTS, Scale::TWO, normal),
            Self::Rotated => Outputs::new(OUTPUTS, Scale::ONE, Transform::Rotated90.into()),
            Self::Flipped => Outputs::new(OUTPUTS, Scale::ONE, Transform::Flipped90.into()),
            Self::UhdBesideFullHd => Outputs::of_sizes(UHD_BESIDE_FULL_HD),
        }
    }

    /// Whether its outputs' sizes are its own, which `--output-size`
    /// cannot change.
    pub(super) fn has_outputs_of_its_own(self) -> bool {
        self == Self::UhdBesideFullHd
    }
}

#[cfg(test)]
mod tests {
    use super::super::suites::SUITES;
    use super::*;

    /// A pass with outputs of its own runs only the tests written for it:
    /// every module a row on such a pass runs is skipped by each of the
    /// same binary's other rows, so no such test also runs on outputs it
    /// was not written for.
    #[test]
    fn the_tests_of_a_pass_with_outputs_of_its_own_run_only_there() {
        for own in SUITES.iter().filter(|suite| {
            suite
                .passes
                .iter()
                .any(|pass| pass.has_outputs_of_its_own())
        }) {
            assert!(
                own.passes.iter().all(|pass| pass.has_outputs_of_its_own()),
                "{:?} mixes passes with outputs of their own and others",
                own.target
            );
            // The modules it runs: its filter's entries that select, not
            // `--skip` nor what follows one.
            let modules: Vec<&str> = own
                .filter
                .iter()
                .enumerate()
                .filter(|&(index, entry)| {
                    *entry != "--skip" && (index == 0 || own.filter[index - 1] != "--skip")
                })
                .map(|(_, entry)| *entry)
                .collect();
            assert!(!modules.is_empty(), "{:?} names no tests", own.target);
            for module in modules {
                for other in SUITES
                    .iter()
                    .filter(|other| other.target == own.target && !std::ptr::eq(*other, own))
                {
                    let skipped = other
                        .filter
                        .windows(2)
                        .any(|pair| pair[0] == "--skip" && pair[1] == module);
                    assert!(
                        skipped,
                        "{:?}: a row runs {module} on other outputs; skip it there",
                        other.target
                    );
                }
            }
        }
    }

    #[test]
    fn the_pass_with_outputs_of_its_own_states_them() {
        let outputs = Pass::UhdBesideFullHd.outputs();
        assert_eq!(outputs.count, 2);
        assert_eq!(outputs.size(0), UHD_BESIDE_FULL_HD[0]);
        assert_eq!(outputs.size(1), UHD_BESIDE_FULL_HD[1]);
        assert!(Pass::UhdBesideFullHd.has_outputs_of_its_own());
        assert!(!Pass::Plain.has_outputs_of_its_own());
    }
}

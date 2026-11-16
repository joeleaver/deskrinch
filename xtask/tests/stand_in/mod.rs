//! Stand-ins for a summoned program, for the measure tests: `sh` scripts
//! that the measurement runs as the program, and the commands that ask it
//! to show.

use std::fs;
use std::path::Path;

/// The stand-in's summon and dismiss: scripts in `dir` that `sh` runs
/// when the measurement runs the program (`sh`) with their paths. The
/// summon asks the stand-in to draw, through a file it watches
/// ([`DRAWS_WHEN_SHOWN`]).
pub(crate) fn summons(dir: &Path) -> (String, String) {
    let summon = dir.join("summon.sh");
    fs::write(&summon, format!("touch {}/show\n", dir.display())).unwrap();
    let dismiss = dir.join("dismiss.sh");
    fs::write(&dismiss, "true\n").unwrap();
    (summon.display().to_string(), dismiss.display().to_string())
}

/// A script loop, `{show}` standing for the file [`summons`]' summon
/// touches, that presents a frame each time it is touched.
pub(crate) const DRAWS_WHEN_SHOWN: &str = "n=0; while :; do if [ -e {show} ]; then rm {show}; \
     n=$((n + 1)); echo \"[PERF] frame $n: paint_frames=1\" >&2; fi; sleep 0.05; done";

/// [`DRAWS_WHEN_SHOWN`] for the stand-in in `dir`.
pub(crate) fn draws_when_shown(dir: &Path) -> String {
    DRAWS_WHEN_SHOWN.replace("{show}", &dir.join("show").display().to_string())
}

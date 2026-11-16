//! Stand-ins for a summoned program, for the measure tests: `sh` scripts
//! that the measurement runs as the program, and the commands that ask it
//! to show.

use std::fs;
use std::path::Path;

/// The stand-in's summon and dismiss: scripts in `dir` that `sh` runs
/// when the measurement runs the program (`sh`) with their paths. The
/// summon asks the stand-in to draw, through a file it watches
/// ([`summoned`]).
pub(crate) fn summons(dir: &Path) -> (String, String) {
    let summon = dir.join("summon.sh");
    fs::write(&summon, format!("touch {}/show\n", dir.display())).unwrap();
    let dismiss = dir.join("dismiss.sh");
    fs::write(&dismiss, "true\n").unwrap();
    (summon.display().to_string(), dismiss.display().to_string())
}

/// A summoned stand-in in `dir`: its summon and dismiss ([`summons`]), and
/// a script that presents a frame each time it is summoned and, if it
/// `keeps_drawing`, one every 0.3 s from its first show on.
pub(crate) struct Summoned {
    pub(crate) summon: String,
    pub(crate) dismiss: String,
    pub(crate) script: String,
}

/// See [`Summoned`].
pub(crate) fn summoned(dir: &Path, keeps_drawing: bool) -> Summoned {
    let (summon, dismiss) = summons(dir);
    let show = dir.join("show").display().to_string();
    let draws = u8::from(keeps_drawing);
    let script = format!(
        "n=0; shown=0; ticks=0; while :; do \
         if [ -e {show} ]; then rm {show}; shown=1; n=$((n + 1)); \
         echo \"[PERF] frame $n: paint_frames=1\" >&2; fi; \
         ticks=$((ticks + 1)); \
         if [ $shown = 1 ] && [ {draws} = 1 ] && [ $((ticks % 6)) = 0 ]; then n=$((n + 1)); \
         echo \"[PERF] frame $n: paint_frames=1\" >&2; fi; \
         sleep 0.05; done"
    );
    Summoned {
        summon,
        dismiss,
        script,
    }
}

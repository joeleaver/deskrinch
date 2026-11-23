//! Stand-ins for a summoned program, for the measure tests: scripts that
//! the measurement runs as the program, and the commands that ask it to
//! show.
//!
//! A stand-in waits as an idle program does, blocked, running nothing: it
//! reads a FIFO for what it is asked, never polling a file. A program that
//! woke twenty times a second would be quiet by its run time but would
//! wait on a run queue at each wake, more the busier the machine, and
//! measure's quiet window counts that wait.

use std::fs;
use std::path::Path;

use crate::fifo::fifo;

/// The stand-in's summon and dismiss: scripts in `dir` that `sh` runs
/// when the measurement runs the program (`sh`) with their paths. The
/// summon writes `s` into the FIFO `dir/asked` ([`fifo`]), which the
/// stand-in reads ([`summoned`]); the dismiss does nothing. The FIFO is
/// made here.
pub(crate) fn summons(dir: &Path) -> (String, String) {
    let asked = fifo(dir, "asked").display().to_string();
    let summon = dir.join("summon.sh");
    fs::write(&summon, format!("printf 's\\n' > {asked}\n")).unwrap();
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

/// See [`Summoned`]. The script is `sh`'s, so that a caller can run lines
/// before it; it ends by handing the process to `bash`, whose `read -t`
/// waits for the next ask or the next frame's time, blocked in between.
pub(crate) fn summoned(dir: &Path, keeps_drawing: bool) -> Summoned {
    let (summon, dismiss) = summons(dir);
    let asked = dir.join("asked").display().to_string();
    let draws = u8::from(keeps_drawing);
    let program = dir.join("stand-in.bash");
    fs::write(
        &program,
        format!(
            "exec 3<>{asked}; n=0; shown=0\n\
             while :; do\n\
             if [ $shown = 1 ] && [ {draws} = 1 ]; then\n\
             read -t 0.3 -u 3 asked || asked=tick\n\
             else\n\
             read -u 3 asked\n\
             fi\n\
             case $asked in\n\
             s) shown=1; n=$((n + 1)); echo \"[PERF] frame $n: paint_frames=1\" >&2 ;;\n\
             tick) n=$((n + 1)); echo \"[PERF] frame $n: paint_frames=1\" >&2 ;;\n\
             esac\n\
             done\n"
        ),
    )
    .unwrap();
    Summoned {
        summon,
        dismiss,
        script: format!("exec bash {}", program.display()),
    }
}

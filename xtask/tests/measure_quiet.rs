//! `cargo xtask measure`'s quiet window end to end, on stand-ins in
//! isolated sessions: a program starved of CPU at its start is not taken
//! for a quiet one, without `--prepares`; and what the compositor itself
//! starts is watched with it (under sway, which `exec`s on request).
//!
//! Starving is done without privilege: the starved process and spinners
//! are pinned to one CPU with `taskset`, the starved one at idle priority
//! (`chrt --idle 0`), and the spinners are killed when the starving is
//! over, so that nothing waits on luck.
//!
//! Ignored by a plain `cargo test`; name the compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure_quiet -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod fifo;
#[cfg(test)]
mod report;
#[cfg(test)]
mod stand_in;

#[cfg(test)]
mod measure_quiet {
    use std::process::{Command, Output, Stdio};

    use crate::fifo::fifo;
    use crate::report::number_after;
    use crate::stand_in::summoned;

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// Starves the shell that runs it: three spinners and the shell pinned
    /// to the last CPU, the shell at idle priority, then `work` (a count),
    /// then the spinners killed and the shell let run anywhere again. At
    /// idle priority beside three spinners it gets about a millisecond of
    /// CPU a second, under the 5 ms a quiet window allows a program to run:
    /// only its state (runnable) and its run-queue wait tell it is starved.
    fn starved(work: &str) -> String {
        format!(
            "cpu=$(( $(nproc) - 1 )); spins=''; \
             for k in 1 2 3; do taskset -c $cpu sh -c 'while :; do :; done' > /dev/null 2>&1 & \
             spins=\"$spins $!\"; done; \
             taskset -p -c $cpu $$ > /dev/null; chrt --idle -p 0 $$; \
             {work}; \
             kill $spins; taskset -p -c 0-$cpu $$ > /dev/null; "
        )
    }

    /// `xtask measure` with `args`, then the program `script` run by `sh`.
    fn measure(args: &[&str], script: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .args(args)
            .args(["sh", "-c", script])
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn a_program_starved_at_its_start_is_not_quiet_without_prepares() {
        // Starved through a count before its first preparation, without
        // --prepares: a settle that read only the program's run time would
        // take that for quiet and summon it before it prepared.
        let dir = tempfile::tempdir().unwrap();
        let stand_in = summoned(dir.path(), false);
        let script = format!(
            "{}echo '[PERF] prepared 1: a' >&2; {}",
            starved("i=0; while [ $i -lt 3000 ]; do i=$((i+1)); done"),
            stand_in.script
        );
        let output = measure(
            &[
                "--summon",
                &stand_in.summon,
                "--dismiss",
                &stand_in.dismiss,
                "--shows",
                "1",
            ],
            &script,
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        let before = number_after(&stdout, "show 1: prepared frames before it, in all");
        assert_eq!(
            before, 1,
            "show 1 was summoned while the program was starved:\n{stdout}"
        );
    }

    /// Under sway: a program that starts hidden, asks sway to start a
    /// child, and is summoned once, its settles watched with that child.
    /// The child is asleep (`spins` false: blocked opening a FIFO) or
    /// spinning (until a file appears). sway starts it in a session of its
    /// own, which the session's process group does not reach, so the
    /// program ends it when it is asked to stop: it creates the file and
    /// opens the FIFO. The program waits a second at a time (one light wake
    /// a second, quiet by the bounds) and draws when the summon's file
    /// appears; its trap runs between two of its sleeps.
    fn with_a_child_of_sway(spins: bool) -> Output {
        let dir = tempfile::tempdir().unwrap();
        let fifo = fifo(dir.path(), "released");
        let stop = dir.path().join("stop");
        let show = dir.path().join("show");
        let (fifo, stop, show) = (fifo.display(), stop.display(), show.display());
        let child = if spins {
            format!("exec sh -c 'while [ ! -e {stop} ]; do :; done'")
        } else {
            format!("exec sh -c 'read x < {fifo}'")
        };
        let script = format!(
            "trap 'touch {stop}; echo > {fifo}; exit 0' TERM; \
             swaymsg -q exec \"{child}\"; n=0; \
             while :; do if [ -e {show} ]; then rm {show}; n=$((n + 1)); \
             echo \"[PERF] frame $n: a\" >&2; fi; sleep 1; done"
        );
        let summon = dir.path().join("summon.sh");
        std::fs::write(&summon, format!("touch {show}\n")).unwrap();
        let dismiss = dir.path().join("dismiss.sh");
        std::fs::write(&dismiss, "true\n").unwrap();
        measure(
            &[
                "--summon",
                &summon.display().to_string(),
                "--dismiss",
                &dismiss.display().to_string(),
                "--shows",
                "1",
                "--first-frame-timeout",
                "15",
            ],
            &script,
        )
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn what_the_compositor_starts_is_watched_with_it() {
        if compositor() != "sway" {
            // Only sway starts a process on a client's request here.
            return;
        }
        // A sleeping child of sway's is in the set and quiet: the hidden
        // settle ends, and the show is measured.
        let output = with_a_child_of_sway(false);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        // A spinning one keeps the hidden settle unquiet, and the failure
        // names it.
        let output = with_a_child_of_sway(true);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains("was not quiet within 15 s"), "{stderr}");
        assert!(stderr.contains("the compositor's child"), "{stderr}");
        assert!(
            stderr.contains("(sh) ran"),
            "the failure does not name the child:\n{stderr}"
        );
    }
}

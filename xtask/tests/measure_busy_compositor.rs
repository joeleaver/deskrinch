//! `cargo xtask measure`'s quiet window waits for a busy compositor: a
//! program hidden and asleep while its compositor still works for another
//! client is not quiet, so the next show is not summoned until the
//! compositor has answered that client.
//!
//! The other client is `cargo xtask busy-compositor`, a fixed amount of
//! plain Wayland work (windows opened, mapped and closed) that writes when
//! the compositor had answered all of it. The dismiss starts it in the
//! background, so the hide's settle runs while it works; the stand-in
//! writes when it is summoned again. The test compares the two moments: the
//! second summon after the busy client was answered. No duration is
//! compared with anything.
//!
//! Its length is bounded on any machine: the busy client's rounds are
//! counted, and it fails, saying the compositor never finished, if they
//! take longer than its limit; the settle's own limit is the first-frame
//! timeout's. The starved row also starves the compositor (every thread
//! pinned beside three spinners at idle priority, killed when the busy
//! client ends, whatever its outcome). KWin cannot be starved so: its file
//! capability puts its threads beyond an unprivileged process's
//! `sched_setscheduler` and `sched_setaffinity` ("Operation not
//! permitted"), so that row runs under sway and labwc.
//!
//! Ignored by a plain `cargo test`; name the compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure_busy_compositor -- --ignored
//! ```

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod fifo;

#[cfg(test)]
mod measure_busy_compositor {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use crate::fifo::fifo;

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// The most seconds the busy client may take.
    const WITHIN: u64 = 120;

    /// Lines that starve the session's compositor until `$spins` are killed.
    const STARVE: &str = "comp=$DESKRINCH_SESSION_COMPOSITOR_PID; cpu=$(( $(nproc) - 1 )); \
                          spins=''; for k in 1 2 3; do \
                          taskset -c $cpu sh -c 'while :; do :; done' > /dev/null 2>&1 & \
                          spins=\"$spins $!\"; done; \
                          taskset -a -c -p $cpu $comp > /dev/null; \
                          for t in /proc/$comp/task/*; do chrt --idle -p 0 ${t##*/}; done; ";

    /// The stand-in, summoned twice, with the busy client of `rounds`
    /// rounds started by the dismiss (after starving the compositor, when
    /// `starved`); the moment the busy client was answered and the moment
    /// of the second summon, in nanoseconds since the Unix epoch, and the
    /// report.
    fn summoned_while_the_compositor_works(
        dir: &Path,
        rounds: u32,
        starved: bool,
    ) -> (u128, u128, String) {
        let asked = fifo(dir, "asked");
        let (asked, d) = (asked.display(), dir.display());
        let summon = dir.join("summon.sh");
        fs::write(&summon, format!("printf 's\\n' > {asked}\n")).unwrap();
        let dismiss = dir.join("dismiss.sh");
        let starve = if starved { STARVE } else { "spins=''; " };
        // Only the first dismiss starts the work: the second show's summon
        // is compared with it. The spinners are killed when the busy client
        // ends, however it ends; they and it are in the session's process
        // group besides.
        fs::write(
            &dismiss,
            format!(
                "[ -e {d}/started ] && exit 0; touch {d}/started\n\
                 {starve}( {xtask} busy-compositor --rounds {rounds} --done {d}/done \
                 --within {WITHIN}; kill $spins ) > {d}/busy.log 2>&1 &\n",
                xtask = env!("CARGO_BIN_EXE_xtask"),
            ),
        )
        .unwrap();
        // Blocked on the FIFO between asks, as an idle program waits.
        let script = format!(
            "exec 3<>{asked}; n=0; while read a <&3; do date +%s%N >> {d}/summoned; \
             n=$((n + 1)); echo \"[PERF] frame $n: paint_frames=1\" >&2; done"
        );
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["measure", "--compositor", &compositor(), "--runs", "1"])
            .arg("--summon")
            .arg(&summon)
            .arg("--dismiss")
            .arg(&dismiss)
            .args(["--shows", "2", "sh", "-c", &script])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let busy = fs::read_to_string(dir.join("busy.log")).unwrap_or_default();
        assert!(
            output.status.success(),
            "{stdout}\n{stderr}\nbusy client: {busy}"
        );
        // A run that ended, its session with it, while the compositor still
        // worked for the busy client summoned show 2 before that too.
        let done = fs::read_to_string(dir.join("done")).unwrap_or_else(|_| {
            panic!(
                "the run ended before the compositor had answered the busy client, so show 2 \
                 was summoned before that too (busy client: {busy:?}):\n{stdout}"
            )
        });
        let summoned = fs::read_to_string(dir.join("summoned")).unwrap();
        let second = summoned.lines().nth(1).expect("a second summon");
        (
            done.trim().parse().unwrap(),
            second.trim().parse().unwrap(),
            stdout.into_owned(),
        )
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_hide_s_settle_waits_for_a_busy_compositor() {
        // 400 rounds: about four seconds of work here, against a settle
        // that, reading the program alone, would end after one.
        let dir = tempfile::tempdir().unwrap();
        let (done, second, report) = summoned_while_the_compositor_works(dir.path(), 400, false);
        assert!(
            second > done,
            "show 2 was summoned {} ms before the compositor had answered the busy client:\n\
             {report}",
            (done - second) / 1_000_000
        );
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_hide_s_settle_waits_for_a_starved_busy_compositor() {
        if compositor() == "kwin" {
            // KWin's threads cannot be starved without privilege (above).
            return;
        }
        // Five rounds, at about a millisecond of CPU a second: seconds.
        let dir = tempfile::tempdir().unwrap();
        let (done, second, report) = summoned_while_the_compositor_works(dir.path(), 5, true);
        assert!(
            second > done,
            "show 2 was summoned {} ms before the starved compositor had answered the busy \
             client:\n{report}",
            (done - second) / 1_000_000
        );
    }
}

//! What the kernel reports, on the machine the test runs on, of the
//! figures `measure`'s quiet window is to rest on: each thread's
//! `schedstat` (run time, run-queue wait) and state, for a stand-in
//! program, the session's compositor and the session's bus, idle and while
//! starved. It prints them (run it with `--nocapture`) and checks only that
//! they are there and move as they must: a sleeping stand-in neither runs
//! nor waits; a starved one is runnable (`R`) and, once it runs, has waited.
//!
//! The starving is done in the session, without privilege: the stand-in
//! (or the compositor) and a spinner are pinned to one CPU with `taskset`,
//! the starved process at idle priority with `chrt --idle 0`; the spinner
//! is killed at the end, so nothing waits on luck.
//!
//! The idle stand-in is sampled only once it has finished starting: once
//! it is `sleep` and asleep (`S`). Sampled at once, a forked child can be
//! part-way through its exec, still running or in `D`. The wait gives up
//! after about 5 s, printing why, and the test fails. The starved stand-in
//! is sampled at once: every step of its start (`taskset`, `chrt`, the
//! starved `sh`) is as runnable as the loop, and at idle priority beside
//! the spinner its exec alone can take seconds, after which it may not run
//! again within the phase to show that it has waited.
//!
//! Ignored by a plain `cargo test`; name the compositor:
//!
//! ```sh
//! DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test quiet_figures -- --ignored --nocapture
//! ```

#[cfg(test)]
mod quiet_figures {
    use std::process::{Command, Stdio};

    /// The compositor to test with.
    fn compositor() -> String {
        std::env::var("DESKRINCH_SESSION_TEST_COMPOSITOR")
            .expect("set DESKRINCH_SESSION_TEST_COMPOSITOR to kwin, sway or labwc")
    }

    /// Run in the session: `sample` prints, every 100 ms,
    /// `LABEL TID STATE RUN WAIT` for each thread of each process given as
    /// `LABEL=PID`; `settle` waits for a process just started to run the
    /// program it was started for, in the state asked, or exits with the
    /// reason.
    const SAMPLE: &str = r#"
sample() { # polls, then LABEL=PID...
    local polls=$1; shift
    for i in $(seq 1 "$polls"); do
        for lp in "$@"; do
            label=${lp%%=*}; pid=${lp#*=}
            for t in /proc/"$pid"/task/*; do
                read -r run wait slices < "$t/schedstat" 2>/dev/null || continue
                state=$(sed 's/.*) //' "$t/stat" 2>/dev/null | cut -d' ' -f1)
                echo "poll $i $label ${t##*/} $state $run $wait"
            done
        done
        sleep 0.1
    done
}
settle() { # PID COMM [STATE]: waits, at most 5 s, until PID runs COMM (in STATE)
    local pid=$1 comm=$2 want=${3:-} now state
    for i in $(seq 1 50); do
        now=$(cat /proc/"$pid"/comm 2>/dev/null)
        state=$(sed 's/.*) //' /proc/"$pid"/stat 2>/dev/null | cut -d' ' -f1)
        if [ "$now" = "$comm" ] && { [ -z "$want" ] || [ "$state" = "$want" ]; }; then
            return 0
        fi
        sleep 0.1
    done
    echo "unsettled: $pid is '$now' in state '$state' after 5 s, not '$comm'${want:+ in state $want}"
    exit 1
}
"#;

    /// The session's processes, then four phases: all idle; a starved
    /// stand-in; the compositor starved; idle again once the spinner is
    /// gone.
    const SCRIPT: &str = r#"
comp=$DESKRINCH_SESSION_COMPOSITOR_PID
bus=$DESKRINCH_SESSION_BUS_PID
cpu=$(( $(nproc) - 1 ))
echo "machine: $(nproc) CPUs, load $(cut -d' ' -f1 /proc/loadavg), kernel $(uname -r)"
echo "sched_schedstats=$(cat /proc/sys/kernel/sched_schedstats 2>&1) task_delayacct=$(cat /proc/sys/kernel/task_delayacct 2>&1)"
echo "compositor $comp $(cat /proc/$comp/comm), bus $bus $(cat /proc/$bus/comm 2>/dev/null)"
echo "compositor's children (task/*/children): $(cat /proc/$comp/task/*/children 2>&1 | tr '\n' ' ')"
echo "tools: $(command -v taskset) $(command -v chrt)"
sleep 300 & idle=$!
settle $idle sleep S
echo "phase idle"
sample 10 stand-in=$idle compositor=$comp bus=$bus
taskset -c $cpu sh -c 'while :; do :; done' & spin=$!
taskset -c $cpu chrt --idle 0 sh -c 'i=0; while [ $i -lt 2000000 ]; do i=$((i+1)); done' & starved=$!
echo "phase starved-stand-in"
sample 10 stand-in=$starved
kill $starved
taskset -a -c -p $cpu $comp > /dev/null
for t in /proc/$comp/task/*; do chrt --idle -p 0 ${t##*/}; done
echo "phase starved-compositor"
sample 10 compositor=$comp
kill $spin
echo "phase idle-again"
sample 10 stand-in=$idle compositor=$comp bus=$bus
kill $idle
echo "done"
"#;

    /// One printed poll line: `(phase, label, state, run, wait)`.
    fn polls(out: &str) -> Vec<(String, String, char, u64, u64)> {
        let mut phase = String::new();
        let mut found = Vec::new();
        for line in out.lines() {
            if let Some(name) = line.strip_prefix("phase ") {
                name.clone_into(&mut phase);
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if let ["poll", _, label, _, state, run, wait] = fields[..] {
                found.push((
                    phase.clone(),
                    label.to_owned(),
                    state.chars().next().unwrap(),
                    run.parse().unwrap(),
                    wait.parse().unwrap(),
                ));
            }
        }
        found
    }

    #[test]
    #[ignore = "needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR"]
    fn the_scheduler_figures_are_there_and_move_as_they_must() {
        let script = format!("{SAMPLE}{SCRIPT}");
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["session", &compositor(), "bash", "-c", &script])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("{stdout}");
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(stdout.contains("\ndone"), "{stdout}\n{stderr}");
        let polls = polls(&stdout);
        let of = |phase: &str, label: &str| -> Vec<_> {
            polls
                .iter()
                .filter(|p| p.0 == phase && p.1 == label)
                .cloned()
                .collect()
        };
        for label in ["stand-in", "compositor", "bus"] {
            assert!(!of("idle", label).is_empty(), "no {label} polls:\n{stdout}");
        }
        // A sleeping stand-in: asleep at every poll, its figures still.
        let idle = of("idle", "stand-in");
        assert!(idle.iter().all(|p| p.2 == 'S'), "{stdout}");
        assert!(
            idle.windows(2)
                .all(|w| w[0].3 == w[1].3 && w[0].4 == w[1].4),
            "{stdout}"
        );
        // A starved one: runnable at most polls, and it has waited.
        let starved = of("starved-stand-in", "stand-in");
        let runnable = starved.iter().filter(|p| p.2 == 'R').count();
        assert!(runnable * 2 > starved.len(), "{stdout}");
        assert!(
            starved.last().unwrap().4 > starved.first().unwrap().4,
            "{stdout}"
        );
    }
}

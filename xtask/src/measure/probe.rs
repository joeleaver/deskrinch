//! One measured start of a program in a session: CPU time to its first
//! frame (per thread, including threads that ended before it), its memory
//! once settled, and an idle window's CPU time, wake-ups and frames.

use std::collections::BTreeMap;
use std::fs;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};

use super::output::Watch;
use super::procfs::{self, ProcessSample, ThreadSample};
use super::report::{Run, Unit, per_minute, per_second_milli};
use super::{gpu, smaps};
use crate::error::Error;
use crate::session::interrupt;

/// How often the threads are sampled before the first frame, so that a
/// thread which ends before it (a shader-compiling worker) is still seen.
const SAMPLE: Duration = Duration::from_millis(2);

/// How long the measurement waits at each step.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    /// For the first frame.
    pub(crate) first_frame: Duration,
    /// After the first frame, before memory is read.
    pub(crate) settle: Duration,
    /// The idle window.
    pub(crate) idle: Duration,
}

/// Starts `command` (a session client) and measures it; ends it afterwards.
pub(crate) fn measure(mut command: Command, program: &str, timing: Timing) -> Result<Run, Error> {
    let start = Instant::now();
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| Error::Spawn {
            program: program.to_owned(),
            source,
        })?;
    let watch = Watch::default();
    let readers = [
        child.stdout.take().map(|s| watch.follow(s)),
        child.stderr.take().map(|s| watch.follow(s)),
    ];
    let result = measure_child(&mut child, &watch, program, start, timing);
    stop(&mut child);
    for reader in readers.into_iter().flatten() {
        // A reader ends when the stream closes, which `stop` caused; one
        // that panicked has nothing more to give.
        reader.join().ok();
    }
    result
}

fn measure_child(
    child: &mut Child,
    watch: &Watch,
    program: &str,
    start: Instant,
    timing: Timing,
) -> Result<Run, Error> {
    let mut run = Run::default();
    startup(&mut run, child, watch, program, start, timing.first_frame)?;
    wait(child, watch, program, timing.settle)?;
    memory(&mut run, child.id(), program)?;
    idle(&mut run, child, watch, program, timing.idle)?;
    run.push("frames in all", Unit::Count, watch.frames());
    Ok(run)
}

/// Samples every thread until the first frame, then records the CPU time
/// to it and the adapter.
fn startup(
    run: &mut Run,
    child: &mut Child,
    watch: &Watch,
    program: &str,
    start: Instant,
    timeout: Duration,
) -> Result<(), Error> {
    let pid = child.id();
    let mut seen: BTreeMap<u32, ThreadSample> = BTreeMap::new();
    while !watch.has_first_frame() {
        interrupt::check()?;
        seen.extend(procfs::read_threads(pid));
        if let Ok(Some(status)) = child.try_wait() {
            return Err(Error::MeasuredExited {
                program: program.to_owned(),
                status,
                output: watch.tail(),
            });
        }
        if start.elapsed() >= timeout {
            return Err(Error::NoFirstFrame {
                program: program.to_owned(),
                waited: timeout,
                output: watch.tail(),
            });
        }
        watch.wait_first_frame(SAMPLE);
    }
    let first = sample(pid, program)?;
    seen.extend(procfs::read_threads(pid));
    let wall = start.elapsed();

    let tick_ns = 1_000_000_000 / clock_ticks_per_second();
    run.push(
        "CPU to first frame, user+system",
        Unit::CpuNs,
        (first.user_ticks + first.system_ticks) * tick_ns,
    );
    run.push("  of it system", Unit::CpuNs, first.system_ticks * tick_ns);
    for (group, ns, count) in procfs::runtime_by_group(seen.values()) {
        run.push(format!("  thread {group}"), Unit::CpuNs, ns);
        run.push(
            format!("    threads named {group}"),
            Unit::Count,
            u64::from(count),
        );
    }
    run.push(
        "threads started by the first frame",
        Unit::Count,
        len(seen.len()),
    );
    run.push("wall time to first frame", Unit::WallNs, nanos(wall));
    run.note("load average at the first frame", load_average());
    let adapter = watch.adapter().map_or_else(
        || "none logged (the software renderer, or no wgpu)".to_owned(),
        |adapter| {
            format!(
                "{} (driver {} {}, {}, {}){}",
                adapter.name,
                adapter.driver,
                adapter.driver_info,
                adapter.device_type,
                adapter.backend,
                if adapter.is_software() {
                    " -- a SOFTWARE Vulkan driver, not the GPU"
                } else {
                    ""
                }
            )
        },
    );
    run.note("GPU adapter", adapter);
    Ok(())
}

/// Records the process's memory: its status sizes, Pss, the resident set by
/// what is mapped, and GPU memory.
fn memory(run: &mut Run, pid: u32, program: &str) -> Result<(), Error> {
    let settled = sample(pid, program)?;
    for field in procfs::STATUS_FIELDS {
        let value = settled.status.get(field).copied().unwrap_or(0);
        let unit = if field == "Threads" {
            Unit::Count
        } else {
            Unit::Kib
        };
        run.push(format!("{field} (settled)"), unit, value);
    }
    let dir = format!("/proc/{pid}");
    let rollup = fs::read_to_string(format!("{dir}/smaps_rollup")).unwrap_or_default();
    run.push(
        "Pss (settled)",
        Unit::Kib,
        procfs::parse_status(&rollup)
            .get("Pss")
            .copied()
            .unwrap_or(0),
    );
    let exe = fs::read_link(format!("{dir}/exe")).unwrap_or_default();
    let maps = fs::read_to_string(format!("{dir}/smaps")).unwrap_or_default();
    let mut groups: Vec<_> = smaps::parse(&maps, &exe.to_string_lossy())
        .into_iter()
        .collect();
    groups.sort_by_key(|(_, usage)| std::cmp::Reverse(usage.rss));
    for (group, usage) in groups.into_iter().filter(|(_, usage)| usage.rss > 0) {
        run.push(format!("  Rss: {group}"), Unit::Kib, usage.rss);
        run.push(format!("  Pss: {group}"), Unit::Kib, usage.pss);
    }
    for (region, (total, resident)) in gpu::sum(&gpu::read(pid)) {
        run.push(
            format!("GPU memory {region}, total (not in Rss)"),
            Unit::Kib,
            total,
        );
        run.push(
            format!("GPU memory {region}, resident"),
            Unit::Kib,
            resident,
        );
    }
    Ok(())
}

/// Records an idle window of `duration`: CPU time, wake-ups, preemptions
/// and frames.
fn idle(
    run: &mut Run,
    child: &mut Child,
    watch: &Watch,
    program: &str,
    duration: Duration,
) -> Result<(), Error> {
    let pid = child.id();
    let before = procfs::read_threads(pid);
    let frames_before = watch.frames();
    let window_start = Instant::now();
    wait(child, watch, program, duration)?;
    let after = procfs::read_threads(pid);
    let millis = u64::try_from(window_start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let frames = watch.frames() - frames_before;
    // A thread that started inside the window counts from zero.
    let delta = |field: fn(&ThreadSample) -> u64| -> u64 {
        after
            .iter()
            .map(|(tid, now)| field(now).saturating_sub(before.get(tid).map_or(0, field)))
            .sum()
    };
    let cpu = delta(|t| t.runtime_ns);
    run.note("idle window", format!("{} s", millis / 1000));
    run.push("frames in the idle window", Unit::Count, frames);
    run.push("idle CPU per minute", Unit::CpuNs, per_minute(cpu, millis));
    run.push(
        "wake-ups per second (voluntary switches)",
        Unit::Milli,
        per_second_milli(delta(|t| t.voluntary_switches), millis),
    );
    run.push(
        "preemptions per second",
        Unit::Milli,
        per_second_milli(delta(|t| t.involuntary_switches), millis),
    );
    if let Some(per_frame) = cpu.checked_div(frames) {
        run.push(
            "CPU per frame in the window (with idle)",
            Unit::CpuNs,
            per_frame,
        );
    }
    Ok(())
}

/// The process now, or why it could not be read.
fn sample(pid: u32, program: &str) -> Result<ProcessSample, Error> {
    procfs::read_process(pid).ok_or_else(|| Error::MeasuredGone {
        program: program.to_owned(),
    })
}

/// Waits `duration`, failing if the program exits or xtask is interrupted.
fn wait(child: &mut Child, watch: &Watch, program: &str, duration: Duration) -> Result<(), Error> {
    let end = Instant::now() + duration;
    while Instant::now() < end {
        interrupt::check()?;
        if let Ok(Some(status)) = child.try_wait() {
            return Err(Error::MeasuredExited {
                program: program.to_owned(),
                status,
                output: watch.tail(),
            });
        }
        thread::sleep((end - Instant::now()).min(Duration::from_millis(100)));
    }
    Ok(())
}

/// Ends the measured program: `SIGTERM`, and `SIGKILL` if it is still
/// running a few seconds later. The session ends whatever it started.
pub(crate) fn stop(child: &mut Child) {
    // It may have exited already; either way it is asked to end.
    let _ = kill_process(Pid::from_child(child), Signal::TERM);
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    // Failing here means it has gone already.
    child.kill().ok();
    child.wait().ok();
}

/// The one-minute load average now, as `/proc/loadavg` gives it.
pub(crate) fn load_average() -> String {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|text| text.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The kernel's clock tick rate (`USER_HZ`), what `/proc/<pid>/stat`
/// counts CPU time in.
fn clock_ticks_per_second() -> u64 {
    rustix::param::clock_ticks_per_second()
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn len(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

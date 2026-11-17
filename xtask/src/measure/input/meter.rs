//! What a step (a key typed, a pointer move) costs, from just before it is
//! taken to settled: CPU time read from `/proc` ([`CpuMeter`]), or
//! instructions from a Callgrind dump of its own ([`CallgrindMeter`]).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use super::super::callgrind;
use super::super::procfs::{self, ThreadSample};
use super::super::report::Unit;
use crate::error::Error;

/// Measures one span at a time.
pub(crate) trait Meter {
    /// What a row of it says was measured, from the step `from` ("key",
    /// "move").
    fn what(&self, from: &str) -> String;
    /// The unit of what it measures.
    fn unit(&self) -> Unit;
    /// Whether a step's wall time is the program's own: so under a CPU
    /// meter, not under Callgrind, whose time is its own.
    fn wall_clock(&self) -> bool;
    /// Starts a span.
    fn start(&mut self) -> Result<(), Error>;
    /// Ends the span, named `name` where the meter keeps one, and gives
    /// what it cost.
    fn stop(&mut self, name: &str) -> Result<u64, Error>;
}

/// The CPU time of the program's threads over a span. A thread that
/// starts and ends inside the span is not seen.
pub(crate) struct CpuMeter {
    pid: u32,
    before: BTreeMap<u32, ThreadSample>,
}

impl CpuMeter {
    /// A meter of process `pid`.
    pub(crate) fn new(pid: u32) -> Self {
        Self {
            pid,
            before: BTreeMap::new(),
        }
    }
}

impl Meter for CpuMeter {
    fn what(&self, from: &str) -> String {
        format!("CPU from the {from} to settled")
    }

    fn unit(&self) -> Unit {
        Unit::CpuNs
    }

    fn wall_clock(&self) -> bool {
        true
    }

    fn start(&mut self) -> Result<(), Error> {
        self.before = procfs::read_threads(self.pid);
        Ok(())
    }

    fn stop(&mut self, _name: &str) -> Result<u64, Error> {
        let after = procfs::read_threads(self.pid);
        Ok(procfs::cpu_between(&self.before, &after))
    }
}

/// The instructions Callgrind counts in the program over a span: zeroed at
/// its start, dumped at its end with the span's name as the hint, and read
/// from the dump that names it.
pub(crate) struct CallgrindMeter<'a> {
    control: &'a dyn Fn() -> Command,
    pid: String,
    out: &'a Path,
}

impl<'a> CallgrindMeter<'a> {
    /// A meter of process `pid`, run under Callgrind writing to `out`,
    /// asked through `control`.
    pub(crate) fn new(control: &'a dyn Fn() -> Command, pid: u32, out: &'a Path) -> Self {
        Self {
            control,
            pid: pid.to_string(),
            out,
        }
    }
}

impl Meter for CallgrindMeter<'_> {
    fn what(&self, from: &str) -> String {
        format!("instructions from the {from} to settled (Ir)")
    }

    fn unit(&self) -> Unit {
        Unit::Count
    }

    fn wall_clock(&self) -> bool {
        false
    }

    fn start(&mut self) -> Result<(), Error> {
        callgrind::control(self.control, &["--zero", &self.pid])
    }

    fn stop(&mut self, name: &str) -> Result<u64, Error> {
        callgrind::dump(self.control, &self.pid, self.out, name)
    }
}

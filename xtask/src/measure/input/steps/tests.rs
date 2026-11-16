use std::io::Cursor;

use super::*;
use crate::measure::report::Unit;

/// A frame line, reactive (`signal_notifies=1`) or a paint alone.
fn frame(n: u64, reactive: bool) -> String {
    let counters = if reactive {
        "paint_frames=1 effect_runs=1 signal_notifies=1"
    } else {
        "style_resolves=1 layout_skipped_paint_only=1 paint_frames=1"
    };
    format!("[PERF] frame {n}: style 0.10ms | {counters}")
}

#[test]
fn a_step_counts_its_main_frames_and_those_that_ran_reactive_work() {
    let lines = [
        frame(6, false),
        "[PERF] prepared 2: style 0.10ms | paint_frames=1 signal_notifies=1".to_owned(),
        "[PERF] window 3 frame 1: style 0.10ms | signal_notifies=2".to_owned(),
        frame(7, true),
        "an ordinary line with signal_notifies=1 in it".to_owned(),
    ];
    assert_eq!(
        Seen::of(&lines),
        Seen {
            presented: 2,
            reactive: 1
        }
    );
    assert_eq!(
        Seen::of(&lines[..1]),
        Seen {
            presented: 1,
            reactive: 0
        }
    );
    let mark = Seen::of(&lines).mark();
    assert!(
        mark.starts_with("1 of its 2 frame(s) ran reactive work"),
        "{mark}"
    );
}

/// A meter whose stop writes a reactive frame into the program's output, as
/// a clock's minute lands while Callgrind writes a step's dump.
struct FrameDuringStop {
    watch: Watch,
}

impl Meter for FrameDuringStop {
    fn what(&self, from: &str) -> String {
        format!("instructions from the {from} to settled (Ir)")
    }

    fn unit(&self) -> Unit {
        Unit::Count
    }

    fn start(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn stop(&mut self, _name: &str) -> Result<u64, Error> {
        let line = format!("{}\n", frame(8, true));
        self.watch
            .follow(Cursor::new(line.into_bytes()))
            .join()
            .unwrap();
        Ok(1_000)
    }
}

#[test]
fn a_frame_that_lands_while_the_dump_is_written_is_counted_in_the_step() {
    let watch = Watch::default();
    watch
        .follow(Cursor::new(format!("{}\n", frame(7, false)).into_bytes()))
        .join()
        .unwrap();
    let mut meter = FrameDuringStop {
        watch: watch.clone(),
    };
    let (cost, seen) = end_step(&mut meter, &watch, 0, "pointer-1").unwrap();
    assert_eq!(cost, 1_000);
    assert_eq!(
        seen,
        Seen {
            presented: 2,
            reactive: 1
        },
        "the frame written during the dump is the step's"
    );
}

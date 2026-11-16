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
    let main = Seen::of(&lines, Scope::Main);
    assert_eq!((main.presented, main.reactive), (2, 1));
    let first = Seen::of(&lines[..1], Scope::Main);
    assert_eq!((first.presented, first.reactive), (1, 0));
}

#[test]
fn a_command_step_counts_every_window_s_frames_and_notes_the_windows_it_opened_and_closed() {
    let lines = [
        "[PERF] window 3 opened: title=\"popup\" app_id=\"x\" output=\"V-0\" output_id=1"
            .to_owned(),
        "[PERF] window 3 frame 1: style 0.10ms | signal_notifies=2".to_owned(),
        frame(7, false),
        "[PERF] window 2 closed".to_owned(),
    ];
    let every = Seen::of(&lines, Scope::Every);
    assert_eq!((every.presented, every.reactive), (2, 1));
    assert_eq!(
        every.windows(),
        "opened window 3 (\"popup\" on V-0); closed window 2"
    );
    assert!(every.shows_a_change());
    // A window closed and no frame is still a change.
    let closed = Seen::of(&lines[3..], Scope::Every);
    assert_eq!(closed.presented, 0);
    assert!(closed.shows_a_change());
    let nothing = Seen::of(&["an ordinary line".to_owned()], Scope::Every);
    assert!(!nothing.shows_a_change());
    assert_eq!(nothing.windows(), "none opened or closed");
    // The main window alone: the other window's frame is not counted.
    assert_eq!(Seen::of(&lines, Scope::Main).presented, 1);
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
    let (cost, seen) = end_step(&mut meter, &watch, 0, "pointer-1", Scope::Main).unwrap();
    assert_eq!(cost, 1_000);
    assert_eq!(
        (seen.presented, seen.reactive),
        (2, 1),
        "the frame written during the dump is the step's"
    );
}

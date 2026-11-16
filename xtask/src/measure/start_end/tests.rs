use std::path::Path;

use super::{End, Start, StartEnds, counted_frame, counted_windows, ordinal};
use crate::measure::windows::Expect;

#[test]
fn each_end_is_read_and_anything_else_refused() {
    assert_eq!(StartEnds::parse("first-frame"), Ok(StartEnds::FirstFrame));
    assert_eq!(StartEnds::parse("frame:2"), Ok(StartEnds::Frame(2)));
    assert_eq!(
        StartEnds::parse("line:all: ready"),
        Ok(StartEnds::Line("all: ready".into()))
    );
    assert_eq!(StartEnds::parse("ready"), Ok(StartEnds::Ready));
    assert_eq!(
        StartEnds::parse("bus-name:org.example.A"),
        Ok(StartEnds::BusName("org.example.A".into()))
    );
    for refused in [
        "",
        "frame",
        "frame:0",
        "frame:x",
        "line:",
        "bus-name:",
        "first",
        "picture:2",
    ] {
        assert!(StartEnds::parse(refused).is_err(), "{refused:?}");
    }
}

#[test]
fn ready_is_the_shell_s_readiness_for_the_component() {
    let launcher = Path::new("/build/release/deskrinch-launcher");
    assert_eq!(
        End::resolve(&StartEnds::Ready, launcher, None).unwrap(),
        End::BusName {
            name: "org.deskrinch.Launcher".into(),
            shell: Some("deskrinch-launcher"),
        }
    );
    // A renamed copy, named by --component.
    let renamed = Path::new("/tmp/measure/launcher-after");
    assert!(End::resolve(&StartEnds::Ready, renamed, None).is_err());
    assert!(End::resolve(&StartEnds::Ready, renamed, Some("deskrinch-launcher")).is_ok());
}

#[test]
fn a_component_ready_once_started_has_no_ready_end_and_is_told_what_it_can_use() {
    let wallpaper = Path::new("/build/release/deskrinch-wallpaper");
    let error = End::resolve(&StartEnds::Ready, wallpaper, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("ready once started"), "{error}");
    assert!(
        error.contains("first-frame, frame:N or line:TEXT"),
        "{error}"
    );
}

#[test]
fn the_default_keeps_the_rows_names_and_any_other_end_names_its_own() {
    let program = Path::new("/bin/true");
    let first = End::resolve(&StartEnds::FirstFrame, program, None).unwrap();
    assert!(first.is_first_frame());
    assert_eq!(first.to(), "first frame");
    assert_eq!(first.at(), "the first frame");
    let second = End::resolve(&StartEnds::Frame(2), program, None).unwrap();
    assert_eq!(second.to(), "start-up's end");
    assert_eq!(
        second.describe(Expect::Windows(2)),
        "each window's 2nd frame"
    );
    assert_eq!(second.describe(Expect::Main), "the main window's 2nd frame");
    let line = End::resolve(&StartEnds::Line("done".into()), program, None).unwrap();
    assert_eq!(
        line.describe(Expect::Main),
        "the first line of its output with \"done\""
    );
}

#[test]
fn a_counter_end_is_the_first_frame_of_each_window_that_carries_it() {
    use deskrinch_test_component::{PerfWindow, Watch};

    assert_eq!(
        StartEnds::parse("counter:image_draws"),
        Ok(StartEnds::Counter("image_draws".into()))
    );
    assert!(StartEnds::parse("counter:").is_err());
    let end = End::resolve(
        &StartEnds::Counter("image_draws".into()),
        Path::new("/bin/true"),
        None,
    )
    .unwrap();
    assert_eq!(
        end.describe(Expect::Windows(2)),
        "each window's first frame with image_draws"
    );
    assert_eq!(
        counted_frame(
            "[PERF] window 2 frame 2: paint 1.0ms | image_draws=1 x=2",
            "image_draws"
        ),
        Some(PerfWindow::Id(2))
    );
    // Absent, or zero, is not counted; nor is a prepared frame.
    assert_eq!(
        counted_frame("[PERF] window 2 frame 1: paint 1.0ms | x=2", "image_draws"),
        None
    );
    assert_eq!(
        counted_frame("[PERF] frame 1: paint 1.0ms | image_draws=0", "image_draws"),
        None
    );
    assert_eq!(
        counted_frame(
            "[PERF] window 1 prepared 1: paint 1.0ms | image_draws=1",
            "image_draws"
        ),
        None
    );
    let watch = Watch::default();
    let lines = "[PERF] window 1 frame 1: a | x=1\n\
                 [PERF] window 2 frame 1: a | image_draws=1\n\
                 [PERF] window 1 frame 2: a | image_draws=2\n";
    watch.follow(lines.as_bytes()).join().expect("reader");
    assert_eq!(
        counted_windows(&watch, "image_draws")
            .into_iter()
            .collect::<Vec<_>>(),
        [PerfWindow::Id(1), PerfWindow::Id(2)]
    );
    let bus = "unused";
    let start = |expect| Start {
        expect,
        end: &end,
        bus,
    };
    assert!(start(Expect::Windows(2)).ended(&watch));
    assert!(!start(Expect::Windows(3)).ended(&watch));
    assert!(!start(Expect::Main).ended(&watch));
}

#[test]
fn ordinals_are_written_as_english_writes_them() {
    let written: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 101, 111]
        .into_iter()
        .map(ordinal)
        .collect();
    assert_eq!(
        written,
        [
            "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "101st", "111th"
        ]
    );
}

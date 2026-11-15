use super::*;

/// A watch that has read `lines`.
fn watch(lines: &str) -> Watch {
    let watch = Watch::default();
    watch
        .follow(std::io::Cursor::new(lines.to_owned()))
        .join()
        .expect("reader");
    watch
}

/// Feeds `lines` to `watch` after what it has read.
fn more(watch: &Watch, lines: &str) {
    watch
        .follow(std::io::Cursor::new(lines.to_owned()))
        .join()
        .expect("reader");
}

const TWO: &str = "[PERF] window 7 opened: title=\"wallpaper\" output=\"HEADLESS-2\"\n\
                   [PERF] window 5 opened: title=\"wallpaper\" output=\"HEADLESS-1\"\n";

#[test]
fn windows_are_named_by_title_and_output_and_the_main_one_as_such() {
    let watch = watch(&format!(
        "{TWO}[PERF] window 9 opened: title=\"no output\"\n[PERF] frame 1: a\n"
    ));
    let names: Vec<String> = watch.windows().iter().map(name).collect();
    assert_eq!(
        names,
        [
            "\"wallpaper\" on HEADLESS-2",
            "\"wallpaper\" on HEADLESS-1",
            "\"no output\"",
            "main window"
        ]
    );
}

#[test]
fn start_up_ends_when_each_window_expected_has_presented() {
    let watch = watch(TWO);
    let expect = Expect::Windows(2);
    assert!(!expect.started(&watch));
    more(&watch, "[PERF] window 7 frame 1: a\n");
    assert!(!expect.started(&watch));
    let error = too_few(&watch.windows(), 2, Duration::from_secs(3)).to_string();
    assert!(
        error.starts_with("1 of the 2 windows expected presented a first frame within 3 s"),
        "{error}"
    );
    assert!(
        error.contains("  \"wallpaper\" on HEADLESS-2: presented\n"),
        "{error}"
    );
    assert!(
        error.ends_with("  \"wallpaper\" on HEADLESS-1: opened, no frame presented"),
        "{error}"
    );
    more(&watch, "[PERF] window 5 frame 1: a\n");
    assert!(expect.started(&watch));
    assert!(
        !Expect::Main.started(&watch),
        "the main window has presented nothing"
    );
}

#[test]
fn frames_in_all_are_every_windows_or_the_main_ones() {
    let watch = watch(&format!(
        "{TWO}[PERF] window 7 frame 1: a\n[PERF] window 5 frame 1: a\n\
         [PERF] window 5 frame 2: a\n[PERF] frame 1: a\n"
    ));
    assert_eq!(Expect::Windows(2).frames(&watch), 4);
    assert_eq!(Expect::Main.frames(&watch), 1);
    assert_eq!(Expect::of(None), Expect::Main);
    assert_eq!(Expect::of(Some(2)), Expect::Windows(2));
}

#[test]
fn more_windows_than_expected_or_two_of_one_name_fail() {
    let three = watch(&format!(
        "{TWO}[PERF] window 8 opened: title=\"wallpaper\" output=\"HEADLESS-3\"\n"
    ));
    let error = Started::of(&three, 2).expect_err("three").to_string();
    assert!(
        error.starts_with("3 windows open where 2 were expected"),
        "{error}"
    );
    let twins = watch(
        "[PERF] window 1 opened: title=\"w\" output=\"HEADLESS-1\"\n\
         [PERF] window 2 opened: title=\"w\" output=\"HEADLESS-1\"\n",
    );
    let error = Started::of(&twins, 2).expect_err("twins").to_string();
    assert_eq!(
        error,
        "two windows are named \"w\" on HEADLESS-1: their rows could not be told apart"
    );
}

#[test]
fn start_up_rows_are_by_name_with_the_remarks_under_the_last() {
    let watch = watch(&format!(
        "{TWO}[PERF] window 7 frame 1: a\n[PERF] window 5 frame 1: a\n"
    ));
    let mut arrivals = Arrivals::default();
    let mut reads = 0;
    arrivals.record(&watch, || {
        reads += 1;
        Some(40)
    });
    arrivals.record(&watch, || unreachable!("no window has come since"));
    assert_eq!(reads, 1);
    let started = Started::of(&watch, 2).expect("two windows");
    let mut run = Run::default();
    let start = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("a second ago");
    started.push_startup(&mut run, &arrivals, &watch.windows(), start);
    let names: Vec<&str> = run.metrics.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "process CPU when \"wallpaper\" on HEADLESS-1 first presented",
            "process CPU when \"wallpaper\" on HEADLESS-2 first presented",
            "wall time when \"wallpaper\" on HEADLESS-1 first presented",
            "wall time when \"wallpaper\" on HEADLESS-2 first presented",
        ]
    );
    assert_eq!(run.metrics[0].value, 40);
    assert_eq!(run.metrics[0].unit, Unit::CpuNs);
    assert_eq!(run.metrics[2].unit, Unit::WallNs);
    let remarked: Vec<&str> = run.remarks.iter().map(|(row, _)| row.as_str()).collect();
    assert_eq!(remarked, [names[1], names[3]]);
    assert!(run.remarks[0].1.starts_with("(cumulative:"));
}

#[test]
fn idle_frames_are_given_in_all_and_per_window() {
    let watch = watch(&format!(
        "{TWO}[PERF] window 7 frame 1: a\n[PERF] window 5 frame 1: a\n"
    ));
    let started = Started::of(&watch, 2).expect("two windows");
    let mark = watch.mark();
    more(
        &watch,
        "[PERF] window 7 frame 2: a\n[PERF] window 7 frame 3: a\n[PERF] window 5 frame 2: a\n",
    );
    let mut run = Run::default();
    assert_eq!(started.push_idle_frames(&mut run, &watch.since(&mark)), 3);
    let rows: Vec<(&str, u64)> = run
        .metrics
        .iter()
        .map(|m| (m.name.as_str(), m.value))
        .collect();
    assert_eq!(
        rows,
        [
            ("frames in the idle window", 3),
            ("  of it \"wallpaper\" on HEADLESS-1", 1),
            ("  of it \"wallpaper\" on HEADLESS-2", 2),
        ]
    );
    assert!(started.unchanged(&watch).is_ok());
}

#[test]
fn a_window_replaced_after_start_up_is_a_change() {
    let watch = watch(&format!(
        "{TWO}[PERF] window 7 frame 1: a\n[PERF] window 5 frame 1: a\n"
    ));
    let started = Started::of(&watch, 2).expect("two windows");
    more(
        &watch,
        "[PERF] window 7 closed\n\
         [PERF] window 8 opened: title=\"wallpaper\" output=\"HEADLESS-2\"\n\
         [PERF] window 9 opened: title=\"brief\"\n[PERF] window 9 closed\n",
    );
    let error = started.unchanged(&watch).expect_err("changed").to_string();
    assert!(
        error.starts_with(
            "its windows changed after start-up:\n  closed: \"wallpaper\" on HEADLESS-2\n  \
             opened: \"wallpaper\" on HEADLESS-2\n  opened: a window, closed again\n"
        ),
        "{error}"
    );
    assert!(error.contains("outside what measure measures"), "{error}");
}

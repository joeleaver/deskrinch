//! The watch's tests: its main-window frames, every window's, its lines.

use super::*;

#[test]
fn counts_frames_and_keeps_the_adapter_and_a_tail() {
    let watch = Watch::default();
    let stderr = "starting\n[PERF] frame 1: a\n[PERF] frame 2: b\n";
    let stdout = "x Request adapter result AdapterInfo { name: \"GPU\", \
                  device_type: IntegratedGpu, driver: \"radv\", backend: Vulkan }\n";
    let handles = [
        watch.follow(stderr.as_bytes()),
        watch.follow(stdout.as_bytes()),
    ];
    for handle in handles {
        handle.join().expect("reader");
    }
    assert!(watch.wait_first_frame(Duration::ZERO).is_some());
    assert!(watch.has_first_frame());
    assert_eq!(watch.frames(), 2);
    assert_eq!(watch.adapter().map(|a| a.driver), Some("radv".to_owned()));
    assert!(!watch.presents_with_software());
    assert!(watch.tail().contains("[PERF] frame 2: b"));
    let lines = watch.lines();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines.contains(&"starting".to_owned()));
}

#[test]
fn waits_for_each_window_s_nth_frame() {
    let watch = Watch::default();
    let stderr = "[PERF] window 1 opened: title=\"a\" app_id=\"x\" output=\"V-0\" output_id=1\n\
                  [PERF] window 2 opened: title=\"a\" app_id=\"x\" output=\"V-1\" output_id=2\n\
                  [PERF] window 1 frame 1: a\n[PERF] window 2 frame 1: a\n\
                  [PERF] window 1 frame 2: a\n";
    watch.follow(stderr.as_bytes()).join().expect("reader");
    assert!(watch.wait_frames_each(2, 1, Duration::ZERO));
    assert!(watch.wait_frames_each(1, 2, Duration::ZERO));
    assert!(!watch.wait_frames_each(2, 2, Duration::ZERO));
    // No main window in a windowless app's lines.
    assert!(!watch.wait_main_frames(1, Duration::ZERO));
    let main = Watch::default();
    main.follow("[PERF] frame 1: a\n[PERF] frame 2: b\n".as_bytes())
        .join()
        .expect("reader");
    assert!(main.wait_main_frames(2, Duration::ZERO));
    assert!(!main.wait_main_frames(3, Duration::ZERO));
}

/// A real run's output (see `perf`'s tests for what wrote it).
const FIXTURE: &str = include_str!("../../tests/fixtures/two-windows-and-a-prepared-one.txt");

#[test]
fn keeps_the_main_windows_frames_apart_from_every_windows() {
    let watch = Watch::default();
    watch.follow(FIXTURE.as_bytes()).join().expect("reader");
    // The main window, as before: its three presented frames; its
    // prepared one neither counted nor its first frame.
    assert_eq!(watch.frames(), 3);
    assert_eq!(watch.prepared(), 1);
    assert!(watch.has_first_frame());
    assert!(watch.wait_prepared_after(0, Duration::ZERO));
    assert!(!watch.wait_prepared_after(1, Duration::ZERO));
    // Every window: three presented, the hidden one only prepared; at
    // the end only the main window is open.
    let first = watch
        .wait_first_frames(3, Duration::ZERO)
        .expect("three windows");
    assert_eq!(first.len(), 3);
    assert!(watch.wait_first_frames(4, Duration::ZERO).is_none());
    let open: Vec<_> = watch.windows().iter().map(|w| w.window).collect();
    assert_eq!(open, [PerfWindow::Main]);
    let mark = watch.mark();
    assert!(
        watch
            .since(&mark)
            .iter()
            .all(|s| s.frames == 0 && s.prepared == 0)
    );
    assert!(!watch.wait_prepared(None, &mark, Duration::ZERO));
}

#[test]
fn waits_for_a_prepared_frame_since_a_mark() {
    let watch = Watch::default();
    let (reader, mut writer) = std::io::pipe().expect("pipe");
    let _follower = watch.follow(reader);
    let mark = watch.mark();
    let later = thread::spawn(move || {
        use std::io::Write as _;
        writer
            .write_all(b"[PERF] window 4 opened: title=\"hidden\"\n[PERF] window 4 prepared 1: a\n")
            .expect("write");
        writer
    });
    let window = Some(PerfWindow::Id(4));
    assert!(watch.wait_prepared(window, &mark, deskrinch_test_wait::PATIENCE));
    assert!(!watch.wait_prepared(Some(PerfWindow::Main), &mark, Duration::ZERO));
    let since = watch.since(&mark);
    assert_eq!(since.len(), 1);
    assert_eq!((since[0].frames, since[0].prepared), (0, 1));
    drop(later.join().expect("writer"));
}

#[test]
fn notes_rinch_declining_the_adapter_wgpu_offered() {
    let watch = Watch::default();
    let stdout = "x Request adapter result AdapterInfo { name: \"llvmpipe\", \
                  device_type: Cpu, driver: \"llvmpipe\", backend: Vulkan }\n\
                  WARN rinch: the only GPU adapter that can present here, \"llvmpipe\" \
                  (driver \"llvmpipe\"), is a software driver that renders on the CPU, \
                  so Renderer::Auto presents with rinch's software renderer instead.\n";
    watch.follow(stdout.as_bytes()).join().expect("reader");
    assert!(watch.adapter().is_some_and(|a| a.is_software()));
    assert!(watch.presents_with_software());
}

#[test]
fn says_which_renderer_presented() {
    let gpu = Watch::default();
    let stdout = "x Request adapter result AdapterInfo { name: \"GPU\", \
                  device_type: IntegratedGpu, driver: \"radv\", backend: Vulkan }\n";
    gpu.follow(stdout.as_bytes()).join().expect("reader");
    assert_eq!(gpu.presented(), None, "before any frame");
    gpu.follow(&b"[PERF] frame 1: a\n"[..])
        .join()
        .expect("reader");
    assert!(matches!(gpu.presented(), Some(Presented::Gpu(a)) if a.driver == "radv"));

    let software = Watch::default();
    let stderr = "INFO rinch: presenting with the software renderer (chosen)\n\
                  [PERF] frame 1: a\n";
    software.follow(stderr.as_bytes()).join().expect("reader");
    assert_eq!(software.presented(), Some(Presented::Software));
}

#[test]
fn a_windowless_app_has_presented_once_another_window_has() {
    let watch = Watch::default();
    let stderr = "INFO rinch: presenting with the software renderer (chosen)\n\
                  [PERF] window 3 opened: title=\"wallpaper\" output=\"HEADLESS-1\"\n";
    watch.follow(stderr.as_bytes()).join().expect("reader");
    assert_eq!(watch.presented(), None, "opened, nothing presented yet");
    watch
        .follow(&b"[PERF] window 3 frame 1: a\n"[..])
        .join()
        .expect("reader");
    assert_eq!(watch.presented(), Some(Presented::Software));
    assert!(!watch.has_first_frame(), "the main window's has not come");
}

#[test]
fn waits_for_a_line_written_before_or_after() {
    let watch = Watch::default();
    watch
        .follow(&b"one\n\x1b[31mtwo\x1b[0m\n"[..])
        .join()
        .expect("reader");
    assert!(watch.wait_for_line(Duration::ZERO, |line| line == "two"));
    let (reader, mut writer) = std::io::pipe().expect("pipe");
    let _follower = watch.follow(reader);
    let later = thread::spawn(move || {
        use std::io::Write as _;
        writer.write_all(b"three\n").expect("write");
    });
    assert!(watch.wait_for_line(deskrinch_test_wait::PATIENCE, |line| line == "three"));
    later.join().expect("writer");
    // Both streams have closed: the wait ends at once, not at its bound.
    let started = Instant::now();
    assert!(!watch.wait_for_line(deskrinch_test_wait::PATIENCE, |line| line == "four"));
    assert!(
        started.elapsed() < deskrinch_test_wait::PATIENCE,
        "it waited {:?} on streams that had closed",
        started.elapsed()
    );
}

#[test]
fn no_first_frame_once_both_streams_close() {
    let watch = Watch::default();
    for handle in [watch.follow(&b"error\n"[..]), watch.follow(&b""[..])] {
        handle.join().expect("reader");
    }
    // Both streams have closed: the wait ends at once, not at its bound.
    let started = Instant::now();
    assert_eq!(watch.wait_first_frame(deskrinch_test_wait::PATIENCE), None);
    assert!(
        started.elapsed() < deskrinch_test_wait::PATIENCE,
        "it waited {:?} on streams that had closed",
        started.elapsed()
    );
    assert_eq!(watch.tail(), "error");
    assert_eq!(watch.last_words(), "error");
}

#[test]
fn the_first_frame_since_a_mark_is_the_main_window_s_or_any_window_s() {
    let watch = Watch::default();
    let stderr = "starting\n[PERF] frame 1: a\nnoise\n\
                  [PERF] window 2 opened: title=\"p\" app_id=\"x\" output=\"V-0\" output_id=1\n\
                  [PERF] window 2 frame 1: a\n[PERF] frame 2: a\n";
    let before = Instant::now();
    watch.follow(stderr.as_bytes()).join().expect("reader");
    let after = Instant::now();
    // From the start: the main window's first frame, line 1.
    let first = watch.first_frame_since(0, false).expect("a main frame");
    assert!(before <= first && first <= after);
    // From line 2 on: the popup's frame (line 4) for any window, the main
    // window's second (line 5) for the main one alone.
    let any = watch
        .first_frame_since(2, true)
        .expect("any window's frame");
    let main = watch.first_frame_since(2, false).expect("a main frame");
    assert!(any <= main, "{any:?} after {main:?}");
    assert_eq!(watch.first_frame_since(6, true), None);
    assert_eq!(watch.first_frame_since(60, true), None);
}

#[test]
fn has_presented_once_any_window_has_closed_or_not() {
    let watch = Watch::default();
    watch
        .follow("starting\n".as_bytes())
        .join()
        .expect("reader");
    assert!(!watch.has_presented());
    let popup = Watch::default();
    popup
        .follow(
            "[PERF] window 2 opened: title=\"p\" app_id=\"x\" output=\"V-0\" output_id=1\n\
             [PERF] window 2 frame 1: a\n[PERF] window 2 closed\n"
                .as_bytes(),
        )
        .join()
        .expect("reader");
    assert!(popup.has_presented(), "a window that closed had presented");
}

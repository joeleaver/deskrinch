//! Watching a rinch program's standard output and error while it runs:
//! counting its frames, noting when the first one came, and keeping the
//! adapter line, whether rinch presented with its software renderer, every
//! line for a test to look for, and the last lines for a report or an
//! error.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::process::Child;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::log::{self, Adapter};

/// How many of the last lines are kept.
const TAIL: usize = 20;
/// How long [`Watch::last_words`] gives an exited program's streams to
/// close.
pub const LAST_WORDS: Duration = Duration::from_secs(2);

/// Which of rinch's renderers presented a program's frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presented {
    /// The software renderer (tiny-skia).
    Software,
    /// The GPU renderer (Vello on wgpu), on this adapter.
    Gpu(Adapter),
}

/// What the program has written so far.
#[derive(Debug, Default)]
struct Seen {
    frames: u64,
    first_frame: Option<Instant>,
    adapter: Option<Adapter>,
    software: bool,
    tail: VecDeque<String>,
    /// Every line, without ANSI codes.
    lines: Vec<String>,
    closed: u8,
}

/// The shared record of a program's output, filled by one reader thread
/// per stream.
#[derive(Debug, Clone, Default)]
pub struct Watch {
    seen: Arc<(Mutex<Seen>, Condvar)>,
}

impl Watch {
    /// Reads `stream` on a thread of its own until it closes.
    pub fn follow(&self, stream: impl Read + Send + 'static) -> JoinHandle<()> {
        let watch = self.clone();
        thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                watch.line(&line);
            }
            let (seen, changed) = &*watch.seen;
            seen.lock().unwrap_or_else(PoisonError::into_inner).closed += 1;
            changed.notify_all();
        })
    }

    /// Follows `child`'s standard output and error, each on a thread of its
    /// own. The threads are not joined: a process the program started may
    /// hold the streams open until its session ends, after the measurement
    /// is done; they end then.
    pub fn follow_child(&self, child: &mut Child) {
        if let Some(stdout) = child.stdout.take() {
            drop(self.follow(stdout));
        }
        if let Some(stderr) = child.stderr.take() {
            drop(self.follow(stderr));
        }
    }

    /// Records one line.
    fn line(&self, line: &str) {
        let (seen, changed) = &*self.seen;
        let mut seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
        if log::frame_number(line).is_some() {
            seen.frames += 1;
            seen.first_frame.get_or_insert_with(Instant::now);
        } else if log::presents_with_software(line) {
            seen.software = true;
        } else if seen.adapter.is_none() {
            seen.adapter = log::adapter(line);
        }
        let plain = log::strip_ansi(line);
        if seen.tail.len() == TAIL {
            seen.tail.pop_front();
        }
        seen.tail.push_back(plain.clone());
        seen.lines.push(plain);
        changed.notify_all();
    }

    /// Waits up to `timeout` for the first frame; returns when it came, or
    /// `None` on timeout or when both streams have closed without one.
    pub fn wait_first_frame(&self, timeout: Duration) -> Option<Instant> {
        let (seen, changed) = &*self.seen;
        let deadline = Instant::now() + timeout;
        let mut guard = seen.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(at) = guard.first_frame {
                return Some(at);
            }
            let now = Instant::now();
            if guard.closed >= 2 || now >= deadline {
                return None;
            }
            // Wake now and then so a caller's interrupt check is not starved.
            let wait = (deadline - now).min(Duration::from_millis(100));
            guard = changed
                .wait_timeout(guard, wait)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Waits up to `timeout` for a frame after the first `frames`; answers
    /// whether one came (not on timeout, or when both streams have closed).
    pub fn wait_frame_after(&self, frames: u64, timeout: Duration) -> bool {
        let (seen, changed) = &*self.seen;
        let deadline = Instant::now() + timeout;
        let mut guard = seen.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if guard.frames > frames {
                return true;
            }
            let now = Instant::now();
            if guard.closed >= 2 || now >= deadline {
                return false;
            }
            guard = changed
                .wait_timeout(guard, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Waits up to `timeout` for a line, written so far or later, that
    /// `wanted` takes; answers whether one came (not on timeout, or when
    /// both streams have closed without it).
    pub fn wait_for_line(&self, timeout: Duration, wanted: impl Fn(&str) -> bool) -> bool {
        let (seen, changed) = &*self.seen;
        let deadline = Instant::now() + timeout;
        let mut guard = seen.lock().unwrap_or_else(PoisonError::into_inner);
        let mut looked = 0;
        loop {
            if guard.lines[looked..].iter().any(|line| wanted(line)) {
                return true;
            }
            looked = guard.lines.len();
            let now = Instant::now();
            if guard.closed >= 2 || now >= deadline {
                return false;
            }
            guard = changed
                .wait_timeout(guard, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// What presented the frames, once the program has said: rinch's
    /// software renderer, or the GPU renderer on the adapter wgpu chose.
    /// `None` before the first frame, and for a program that presented
    /// without saying either (a build without `RUST_LOG` letting the lines
    /// through).
    #[must_use]
    pub fn presented(&self) -> Option<Presented> {
        let seen = self.lock();
        if seen.first_frame.is_none() {
            None
        } else if seen.software {
            Some(Presented::Software)
        } else {
            seen.adapter.clone().map(Presented::Gpu)
        }
    }

    /// Whether the first frame has come.
    #[must_use]
    pub fn has_first_frame(&self) -> bool {
        self.lock().first_frame.is_some()
    }

    /// The frames counted so far.
    #[must_use]
    pub fn frames(&self) -> u64 {
        self.lock().frames
    }

    /// The adapter wgpu logged, if it did.
    #[must_use]
    pub fn adapter(&self) -> Option<Adapter> {
        self.lock().adapter.clone()
    }

    /// Whether rinch said it presents with its software renderer.
    #[must_use]
    pub fn presents_with_software(&self) -> bool {
        self.lock().software
    }

    /// What presented the frames, for a report: rinch's software renderer,
    /// or the GPU renderer on the adapter wgpu named, marked when that is a
    /// software Vulkan driver.
    #[must_use]
    pub fn renderer(&self) -> String {
        let Some(adapter) = self.adapter() else {
            return "software (no GPU adapter logged)".to_owned();
        };
        let named = format!(
            "{} (driver {} {}, {}, {})",
            adapter.name, adapter.driver, adapter.driver_info, adapter.device_type, adapter.backend,
        );
        if self.presents_with_software() {
            format!("software; rinch did not use the adapter wgpu offered, {named}")
        } else if adapter.is_software() {
            format!("GPU on {named} -- a SOFTWARE Vulkan driver, not the GPU")
        } else {
            format!("GPU on {named}")
        }
    }

    /// The last lines of a program that has exited: [`tail`](Self::tail)
    /// once both streams have closed, so that what it wrote just before
    /// exiting is read, or after [`LAST_WORDS`] if something it started
    /// still holds them open.
    pub fn last_words(&self) -> String {
        let (seen, changed) = &*self.seen;
        let deadline = Instant::now() + LAST_WORDS;
        let mut guard = seen.lock().unwrap_or_else(PoisonError::into_inner);
        while guard.closed < 2 {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            guard = changed
                .wait_timeout(guard, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        drop(guard);
        self.tail()
    }

    /// The last lines written, oldest first.
    #[must_use]
    pub fn tail(&self) -> String {
        self.lock()
            .tail
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Seen> {
        self.seen.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
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
        assert!(watch.wait_for_line(Duration::from_secs(600), |line| line == "three"));
        later.join().expect("writer");
        assert!(!watch.wait_for_line(Duration::from_millis(50), |line| line == "four"));
    }

    #[test]
    fn no_first_frame_once_both_streams_close() {
        let watch = Watch::default();
        for handle in [watch.follow(&b"error\n"[..]), watch.follow(&b""[..])] {
            handle.join().expect("reader");
        }
        assert_eq!(watch.wait_first_frame(Duration::from_secs(60)), None);
        assert_eq!(watch.tail(), "error");
        assert_eq!(watch.last_words(), "error");
    }
}

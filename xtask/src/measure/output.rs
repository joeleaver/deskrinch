//! Watching the measured program's standard output and error while it
//! runs: counting its frames, noting when the first one came, and keeping
//! the adapter line and the last lines for a report or an error.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::client_log::{self, Adapter};

/// How many of the last lines are kept.
const TAIL: usize = 20;

/// What the program has written so far.
#[derive(Debug, Default)]
struct Seen {
    frames: u64,
    first_frame: Option<Instant>,
    adapter: Option<Adapter>,
    tail: VecDeque<String>,
    closed: u8,
}

/// The shared record of a program's output, filled by one reader thread
/// per stream.
#[derive(Debug, Clone, Default)]
pub(crate) struct Watch {
    seen: Arc<(Mutex<Seen>, Condvar)>,
}

impl Watch {
    /// Reads `stream` on a thread of its own until it closes.
    pub(crate) fn follow(&self, stream: impl Read + Send + 'static) -> JoinHandle<()> {
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

    /// Records one line.
    fn line(&self, line: &str) {
        let (seen, changed) = &*self.seen;
        let mut seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
        if client_log::frame_number(line).is_some() {
            seen.frames += 1;
            seen.first_frame.get_or_insert_with(Instant::now);
        } else if seen.adapter.is_none() {
            seen.adapter = client_log::adapter(line);
        }
        if seen.tail.len() == TAIL {
            seen.tail.pop_front();
        }
        seen.tail.push_back(client_log::strip_ansi(line));
        changed.notify_all();
    }

    /// Waits up to `timeout` for the first frame; returns when it came, or
    /// `None` on timeout or when both streams have closed without one.
    pub(crate) fn wait_first_frame(&self, timeout: Duration) -> Option<Instant> {
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

    /// Whether the first frame has come.
    pub(crate) fn has_first_frame(&self) -> bool {
        self.lock().first_frame.is_some()
    }

    /// The frames counted so far.
    pub(crate) fn frames(&self) -> u64 {
        self.lock().frames
    }

    /// The adapter wgpu logged, if it did.
    pub(crate) fn adapter(&self) -> Option<Adapter> {
        self.lock().adapter.clone()
    }

    /// The last lines written, oldest first.
    pub(crate) fn tail(&self) -> String {
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
        assert!(watch.tail().contains("[PERF] frame 2: b"));
    }

    #[test]
    fn no_first_frame_once_both_streams_close() {
        let watch = Watch::default();
        for handle in [watch.follow(&b"error\n"[..]), watch.follow(&b""[..])] {
            handle.join().expect("reader");
        }
        assert_eq!(watch.wait_first_frame(Duration::from_secs(60)), None);
        assert_eq!(watch.tail(), "error");
    }
}

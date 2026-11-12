//! Where KWin writes a screenshot's pixels: a memory file, not a pipe.
//!
//! What follows was read in KWin's source, at the `v6.6.6` tag (the KWin
//! this machine and CI's Ubuntu 26.04 run) and on the `Plasma/6.6` branch,
//! which agree; it was not watched happening under a debugger.
//!
//! KWin's writer (`ScreenShotWriter2::run`, in
//! `src/plugins/screenshot/screenshotdbusinterface2.cpp`) sets `O_NONBLOCK`
//! on the descriptor it is given and writes through a buffered `QFile`
//! (opened `QIODevice::WriteOnly`, without `Unbuffered`, and never flushed),
//! stopping at `if (writtenCount == 0 || remainingSize == 0)`. The buffer's
//! last bytes are written when the `QFile` is destroyed; into a pipe whose
//! reader is behind (under load), that write fails with `EAGAIN` and the
//! bytes are lost. KWin has answered the call already and only logs the
//! failure, so the reader saw a clean end of file, short of what the answer
//! described.
//!
//! A memory file never fills: every write KWin makes lands whole. End of
//! file means nothing for a file, so the end of the writing is the file
//! being closed: `IN_CLOSE_WRITE` on it, which comes once every descriptor
//! of the write side is closed, ours and KWin's.

use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use deskrinch_test_wayland::{Deadline, memory_file};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::inotify::{self, CreateFlags, WatchFlags};
use rustix::fs::{Mode, OFlags, open};

/// A memory file KWin writes the pixels into, and the watch on its writing.
#[derive(Debug)]
pub(crate) struct Sink {
    /// The file, read once the writing has ended.
    file: OwnedFd,
    /// Our descriptor of the write side, passed to KWin; dropped after the
    /// call, so that only KWin's copy keeps the write side open.
    writer: Option<OwnedFd>,
    /// Hears `IN_CLOSE_WRITE` on the file.
    watch: OwnedFd,
}

impl Sink {
    /// A new, empty memory file, its write side open, and a watch on it.
    pub(crate) fn new() -> io::Result<Self> {
        let file = OwnedFd::from(memory_file("deskrinch-screenshot")?);
        let path = format!("/proc/self/fd/{}", rustix::fd::AsRawFd::as_raw_fd(&file));
        // A write side of its own (a description of its own), so that the
        // file's own descriptor, which stays open for reading, never holds
        // the write side open.
        let writer = open(&path, OFlags::WRONLY | OFlags::CLOEXEC, Mode::empty())?;
        // The watch exists before the write side can leave our hands
        // (`writer` hands it out only once this returns): a writer quick
        // enough to close it before a watch was set would leave no event,
        // and the wait would last until its deadline. A close after this
        // point is queued on the watch, however soon the wait begins.
        let watch = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK)?;
        inotify::add_watch(&watch, &path, WatchFlags::CLOSE_WRITE)?;
        Ok(Self {
            file,
            writer: Some(writer),
            watch,
        })
    }

    /// The write side, for passing to KWin.
    pub(crate) fn writer(&self) -> Option<BorrowedFd<'_>> {
        self.writer.as_ref().map(AsFd::as_fd)
    }

    /// Drops our descriptor of the write side: from now on only the one
    /// passed away keeps it open.
    pub(crate) fn close_writer(&mut self) {
        self.writer = None;
    }

    /// Waits until the write side is closed (by every holder: call
    /// [`close_writer`](Self::close_writer) first), at most until `deadline`
    /// (an error of kind `TimedOut`), and returns what was written.
    pub(crate) fn wait_written(&self, deadline: Deadline) -> io::Result<Vec<u8>> {
        loop {
            let left = deadline.left();
            if left.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let timeout = Timespec::try_from(left).unwrap_or(Timespec {
                tv_sec: i64::MAX,
                tv_nsec: 0,
            });
            let mut fds = [PollFd::new(&self.watch, PollFlags::IN)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) | Err(rustix::io::Errno::INTR) => continue,
                Ok(_) => {}
                Err(errno) => return Err(errno.into()),
            }
            if self.closed()? {
                return self.contents();
            }
        }
    }

    /// Whether the watch has heard the write side close.
    fn closed(&self) -> io::Result<bool> {
        let mut buffer = [std::mem::MaybeUninit::uninit(); 1024];
        let mut reader = inotify::Reader::new(&self.watch, &mut buffer);
        let mut closed = false;
        loop {
            match reader.next() {
                Ok(event) => closed |= event.events().contains(inotify::ReadFlags::CLOSE_WRITE),
                Err(rustix::io::Errno::AGAIN) => return Ok(closed),
                Err(rustix::io::Errno::INTR) => {}
                Err(errno) => return Err(errno.into()),
            }
        }
    }

    /// Everything written to the file.
    fn contents(&self) -> io::Result<Vec<u8>> {
        let size = rustix::fs::fstat(&self.file)?.st_size;
        let size = usize::try_from(size).map_err(io::Error::other)?;
        let mut bytes = vec![0; size];
        let mut done = 0;
        while done < size {
            let offset = u64::try_from(done).map_err(io::Error::other)?;
            match rustix::io::pread(&self.file, &mut bytes[done..], offset) {
                Ok(0) => break,
                Ok(read) => done += read,
                Err(rustix::io::Errno::INTR) => {}
                Err(errno) => return Err(errno.into()),
            }
        }
        bytes.truncate(done);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write as _;
    use std::thread;
    use std::time::Duration;

    use rustix::fs::{fcntl_getfl, fcntl_setfl};

    use super::*;

    /// A writer that, as KWin's does, makes the description non-blocking
    /// and writes a picture in pieces with pauses between them.
    fn write_in_pieces(writer: OwnedFd, picture: &[u8]) {
        let flags = fcntl_getfl(&writer).unwrap();
        fcntl_setfl(&writer, flags | OFlags::NONBLOCK).unwrap();
        let mut file = File::from(writer);
        for piece in picture.chunks(picture.len() / 7 + 1) {
            file.write_all(piece).unwrap();
            thread::sleep(Duration::from_millis(30));
        }
    }

    #[test]
    fn a_picture_written_in_pieces_is_read_whole() {
        let mut sink = Sink::new().unwrap();
        // KWin's copy: the same description, as a descriptor passed over the
        // bus is.
        let theirs = sink.writer().unwrap().try_clone_to_owned().unwrap();
        // 1280×720 32-bit pixels: far more than a pipe holds.
        let picture: Vec<u8> = (0..1280 * 720 * 4_u32)
            .map(|n| u8::try_from(n % 251).unwrap())
            .collect();
        let sent = picture.clone();
        let writing = thread::spawn(move || write_in_pieces(theirs, &sent));
        sink.close_writer();
        let read = sink
            .wait_written(Deadline::after(Duration::from_secs(600)))
            .unwrap();
        writing.join().unwrap();
        assert_eq!(read.len(), picture.len());
        assert!(read == picture, "the bytes are the ones written");
    }

    #[test]
    fn a_writer_done_before_the_wait_begins_is_read_at_once() {
        let mut sink = Sink::new().unwrap();
        let theirs = sink.writer().unwrap().try_clone_to_owned().unwrap();
        // The writer writes and closes before anyone waits, as a fast KWin
        // may.
        write_in_pieces(theirs, b"a whole picture");
        sink.close_writer();
        let started = std::time::Instant::now();
        let read = sink
            .wait_written(Deadline::after(Duration::from_secs(600)))
            .unwrap();
        assert_eq!(read, b"a whole picture");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "read at once, not at the deadline"
        );
    }

    #[test]
    fn nothing_is_read_while_a_writer_still_holds_it() {
        let mut sink = Sink::new().unwrap();
        let theirs = sink.writer().unwrap().try_clone_to_owned().unwrap();
        sink.close_writer();
        let error = sink
            .wait_written(Deadline::after(Duration::from_millis(200)))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        drop(theirs);
        let read = sink
            .wait_written(Deadline::after(Duration::from_secs(600)))
            .unwrap();
        assert_eq!(read, Vec::<u8>::new());
    }
}

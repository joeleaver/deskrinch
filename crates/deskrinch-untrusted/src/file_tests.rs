use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};

use super::*;

#[test]
fn a_regular_file_within_the_bound_is_read_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a");
    std::fs::write(&path, b"12345").unwrap();
    assert_eq!(read_bounded(&path, 5).unwrap(), b"12345");
    assert_eq!(read_bounded(&path, 100).unwrap(), b"12345");
    std::fs::write(&path, b"").unwrap();
    assert_eq!(read_bounded(&path, 0).unwrap(), b"");
}

#[test]
fn a_file_over_the_bound_is_refused_from_its_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a");
    std::fs::write(&path, b"123456").unwrap();
    assert!(matches!(
        read_bounded(&path, 5),
        Err(FileError::TooLarge { size: 6, max: 5 })
    ));
}

/// A file whose size, at the check, is less than what it then holds: as a
/// file that grew after the `fstat`. `/proc` files say they are empty and
/// hold their text.
#[test]
fn a_file_that_holds_more_than_its_size_said_is_refused_at_the_bound() {
    let path = Path::new("/proc/self/status");
    assert_eq!(std::fs::metadata(path).unwrap().len(), 0);
    assert!(matches!(
        read_bounded(path, 16),
        Err(FileError::TooLarge { size: 17, max: 16 })
    ));
    assert!(read_bounded(path, 1 << 20).unwrap().len() > 16);
}

#[test]
fn a_directory_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        read_bounded(dir.path(), 100),
        Err(FileError::Directory)
    ));
}

#[test]
fn a_fifo_with_no_writer_is_refused_without_blocking() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pipe");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &path,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    let started = Instant::now();
    assert!(matches!(read_bounded(&path, 100), Err(FileError::Fifo)));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_device_is_refused() {
    assert!(matches!(
        read_bounded(Path::new("/dev/null"), 100),
        Err(FileError::Device)
    ));
    assert!(matches!(
        read_bounded(Path::new("/dev/zero"), 100),
        Err(FileError::Device)
    ));
}

#[test]
fn a_socket_cannot_be_opened() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("socket");
    let _listener = UnixListener::bind(&path).unwrap();
    assert!(matches!(read_bounded(&path, 100), Err(FileError::Io(_))));
}

#[test]
fn a_missing_file_is_an_io_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        read_bounded(&dir.path().join("missing"), 100),
        Err(FileError::Io(_))
    ));
}

#[test]
fn a_link_to_a_regular_file_is_read_and_a_link_to_a_fifo_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a");
    std::fs::write(&file, b"x").unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    assert_eq!(read_bounded(&link, 1).unwrap(), b"x");
    let pipe = dir.path().join("pipe");
    rustix::fs::mkfifoat(rustix::fs::CWD, &pipe, rustix::fs::Mode::RUSR).unwrap();
    let to_pipe = dir.path().join("to-pipe");
    std::os::unix::fs::symlink(&pipe, &to_pipe).unwrap();
    assert!(matches!(read_bounded(&to_pipe, 1), Err(FileError::Fifo)));
}

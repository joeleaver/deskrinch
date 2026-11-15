//! Each case on a socket made to order: bound and not listened on, which
//! is a socket file nothing accepts on, as a compositor's is between its
//! `bind` and its `listen` and after it has died. No case waits on the
//! clock: the deadlines are either never reached or already passed.

use std::os::fd::OwnedFd;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::time::Duration;

use rustix::net::{AddressFamily, SocketAddrUnix, SocketType, bind, listen, socket};

use super::*;

/// A socket bound at `path` and not listened on.
fn bound(path: &Path) -> OwnedFd {
    let fd = socket(AddressFamily::UNIX, SocketType::STREAM, None).unwrap();
    bind(&fd, &SocketAddrUnix::new(path).unwrap()).unwrap();
    fd
}

/// A deadline that will not pass while a test runs.
fn far() -> Deadline {
    Deadline::patience()
}

/// A deadline already passed.
fn passed() -> Deadline {
    Deadline::after(Duration::ZERO)
}

#[test]
fn a_socket_not_yet_listened_on_is_tried_again_until_it_accepts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wayland-1");
    let fd = bound(&path);
    let mut calls = 0;
    let connected = connect_when_accepting(&path, far(), || {
        calls += 1;
        if calls == 2 {
            listen(&fd, 1).unwrap();
        }
        Ok::<(), Infallible>(())
    });
    assert!(connected.is_ok(), "{connected:?}");
    assert_eq!(calls, 2, "refused twice, then accepted");
}

#[test]
fn a_compositor_that_exited_stops_the_tries_with_its_status() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wayland-1");
    let _fd = bound(&path);
    let exited = ExitStatus::from_raw(3 << 8);
    let mut calls = 0;
    let connected = connect_when_accepting(&path, far(), || {
        calls += 1;
        Err(exited)
    });
    match connected {
        Err(Connecting::Stopped(status)) => assert_eq!(status.code(), Some(3)),
        other => panic!("not stopped with the status: {other:?}"),
    }
    assert_eq!(calls, 1, "tried once, then asked");
}

#[test]
fn a_socket_refusing_past_the_deadline_is_not_accepting_after_one_try() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wayland-1");
    let _fd = bound(&path);
    let connected = connect_when_accepting(&path, passed(), || -> Result<(), Infallible> {
        panic!("asked to wait although the deadline had passed")
    });
    match connected {
        Err(Connecting::NotAccepting { socket, last }) => {
            assert_eq!(socket, path);
            assert_eq!(last.kind(), io::ErrorKind::ConnectionRefused);
        }
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn no_socket_file_yet_is_tried_again_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wayland-1");
    let mut fd = None;
    let connected = connect_when_accepting(&path, far(), || {
        // The compositor binds and listens between the first and second try.
        let made = bound(&path);
        listen(&made, 1).unwrap();
        fd = Some(made);
        Ok::<(), Infallible>(())
    });
    assert!(connected.is_ok(), "{connected:?}");
    assert!(fd.is_some());
}

#[test]
fn one_try_reports_a_refusal_as_not_accepting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wayland-1");
    let fd = bound(&path);
    match connect_once(&path) {
        Err(Connecting::NotAccepting { last, .. }) => {
            assert_eq!(last.kind(), io::ErrorKind::ConnectionRefused);
        }
        other => panic!("not refused: {other:?}"),
    }
    listen(&fd, 1).unwrap();
    assert!(connect_once(&path).is_ok());
    let error = connect_once(&dir.path().join("wayland-9")).unwrap_err();
    assert!(
        matches!(error, Connecting::NotAccepting { .. }),
        "{error:?}"
    );
}

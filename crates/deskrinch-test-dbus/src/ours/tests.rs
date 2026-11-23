//! The proof's makers refuse, in-process and with no bus anywhere; that
//! the public `here` reads the process's own environment, and that what
//! `at` refuses is never connected to, is tested in a child of this test
//! binary outside any session.

use std::process::{Command, Stdio};

use deskrinch_test_session::{BUS_VARIABLE, KWIN, root};

use super::*;
use crate::session::outside;

#[test]
fn outside_a_session_here_refuses_saying_why() {
    let refusal = OurBus::in_session(&Environment::default()).unwrap_err();
    assert_eq!(refusal, NotOurs::NotIsolated(NotIsolated::NoCompositor));
    assert!(
        refusal
            .to_string()
            .starts_with("not inside an isolated session"),
        "{refusal}"
    );
}

/// The owner's own session, with a compositor named by hand.
#[test]
fn the_real_session_with_a_compositor_named_is_refused() {
    let real = Environment {
        compositor: Some(KWIN.into()),
        session_dir: None,
        bus: Some("unix:path=/run/user/1000/bus".into()),
    };
    assert_eq!(
        OurBus::in_session(&real),
        Err(NotOurs::NotIsolated(NotIsolated::NoSessionDir))
    );
}

/// A session of ours, under any compositor: the proof is for its bus, and
/// the check's finding comes with it.
#[test]
fn a_session_of_ours_proves_its_bus() {
    let root = tempfile::Builder::new()
        .prefix(root::PREFIX)
        .tempdir_in(root::PARENT)
        .unwrap();
    root::mark(root.path()).unwrap();
    let address = bus::listen_address(&bus::socket_name(1, 0));
    let session = Environment {
        compositor: Some(deskrinch_test_session::SWAY.into()),
        session_dir: Some(root.path().as_os_str().to_owned()),
        bus: Some(address.clone().into()),
    };
    let (ours, isolated) = OurBus::in_session(&session).unwrap();
    assert_eq!(ours.address(), address);
    assert_eq!(isolated.compositor, deskrinch_test_session::SWAY);
}

/// Every address `at` takes or refuses: only a private bus of ours.
#[test]
fn at_takes_only_a_private_bus_of_ours() {
    let ours = bus::listen_address(&bus::socket_name(42, 3));
    assert_eq!(OurBus::at(&ours).unwrap().address(), ours);
    for address in [
        // The real session's bus.
        "unix:path=/run/user/1000/bus",
        // A list, whose second entry is the real bus.
        "unix:abstract=/tmp/dbus-deskrinch-1-0;unix:path=/run/user/1000/bus",
        // Another abstract name.
        "unix:abstract=/tmp/dbus-AbCdEf",
        // Our name, as a path.
        "unix:path=/tmp/dbus-deskrinch-1-0",
        "",
    ] {
        assert_eq!(
            OurBus::at(address),
            Err(NotOurs::NotPrivate {
                address: address.to_owned()
            }),
            "{address}"
        );
    }
    assert_eq!(
        OurBus::at("unix:path=/run/user/1000/bus")
            .unwrap_err()
            .to_string(),
        "unix:path=/run/user/1000/bus is not a private bus of ours"
    );
}

/// Set by the outer tests for the inner ones, which do nothing without it.
const INNER: &str = "DESKRINCH_TEST_DBUS_INNER";

/// The inner test `name` run in a child of this test binary, outside any
/// session; what it said, having failed.
fn refused_in_a_child(name: &str) -> String {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args(["--ignored", "--exact", "--test-threads=1", name])
        .env(INNER, "1")
        .stdin(Stdio::null());
    let _root = outside(&mut child).unwrap();
    let output = child.output().unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("1 failed"), "{name} did not fail: {said}");
    said
}

#[test]
fn here_refuses_outside_a_session_before_any_bus() {
    let said = refused_in_a_child("ours::tests::inner::connects_here");
    assert!(said.contains("not inside an isolated session"), "{said}");
}

/// The address the child gives `at` is the one `outside` gives it: a
/// `unix:path=` address, as the real session's is, with nothing listening,
/// so that a maker that took it would reach nothing.
#[test]
fn at_refuses_an_address_not_ours_before_any_bus() {
    let said = refused_in_a_child("ours::tests::inner::connects_at_the_session_s_address");
    assert!(said.contains("is not a private bus of ours"), "{said}");
}

/// The public makers, run only by the outer tests above in their child;
/// each connects with the proof, should it have been made.
mod inner {
    use super::*;

    fn connect(made: Result<OurBus, NotOurs>) {
        let ours = made.unwrap_or_else(|why| panic!("{why}"));
        if let Err(why) = zbus::block_on(ours.connect()) {
            panic!("connected and failed: {why}");
        }
    }

    #[test]
    #[ignore = "run by its outer test, in a child outside any session"]
    fn connects_here() {
        if std::env::var_os(INNER).is_some() {
            connect(OurBus::here());
        }
    }

    #[test]
    #[ignore = "run by its outer test, in a child outside any session"]
    fn connects_at_the_session_s_address() {
        if std::env::var_os(INNER).is_some() {
            connect(OurBus::at(&std::env::var(BUS_VARIABLE).unwrap()));
        }
    }
}

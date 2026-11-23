//! The handle's two makers refuse, in-process and with no bus anywhere:
//! `here` for an environment that is not an isolated KWin session's, `on`
//! for an address that is not a private bus of ours. That the public makers
//! read the process's own environment is tested in a child
//! (`wiring_tests`).

use deskrinch_test_session::{NotIsolated, bus, root};

use super::*;
use crate::error::{Act, Refusal};

#[test]
fn outside_a_session_here_refuses_saying_why() {
    let refusal = Kwin::in_environment(&Environment::default()).unwrap_err();
    assert!(
        matches!(
            refusal,
            Error::Refused {
                act: Act::Ask,
                why: Refusal::NotIsolated(NotIsolated::NoCompositor)
            }
        ),
        "{refusal:?}"
    );
    assert!(
        refusal
            .to_string()
            .starts_with("refusing to ask KWin: not inside an isolated session"),
        "{refusal}"
    );
}

/// The owner's own session, with the compositor's name set by hand: the
/// name alone is not a session.
#[test]
fn the_real_session_with_kwin_named_is_refused() {
    let real = Environment {
        compositor: Some(KWIN.into()),
        session_dir: None,
        bus: Some("unix:path=/run/user/1000/bus".into()),
    };
    assert!(matches!(
        Kwin::in_environment(&real),
        Err(Error::Refused {
            why: Refusal::NotIsolated(NotIsolated::NoSessionDir),
            ..
        })
    ));
}

/// A session of ours: under sway refused, under KWin the handle is for its
/// bus.
#[test]
fn a_session_of_ours_is_kwin_s_only_under_kwin() {
    let root = tempfile::Builder::new()
        .prefix(root::PREFIX)
        .tempdir_in(root::PARENT)
        .unwrap();
    root::mark(root.path()).unwrap();
    let address = bus::listen_address(&bus::socket_name(1, 0));
    let session = |compositor: &str| Environment {
        compositor: Some(compositor.into()),
        session_dir: Some(root.path().as_os_str().to_owned()),
        bus: Some(address.clone().into()),
    };
    let refusal = Kwin::in_environment(&session(deskrinch_test_session::SWAY)).unwrap_err();
    assert!(
        matches!(
            &refusal,
            Error::Refused { why: Refusal::NotKwin { compositor }, .. }
                if compositor == deskrinch_test_session::SWAY
        ),
        "{refusal:?}"
    );
    let kwin = Kwin::in_environment(&session(KWIN)).unwrap();
    assert_eq!(kwin.bus.address(), address);
}

/// Every address `on` takes or refuses: only a private bus of ours.
#[test]
fn on_takes_only_a_private_bus_of_ours() {
    let ours = bus::listen_address(&bus::socket_name(42, 3));
    assert_eq!(Kwin::on(&ours).unwrap().bus.address(), ours);
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
        let refusal = Kwin::on(address).unwrap_err();
        assert!(
            matches!(
                &refusal,
                Error::Refused { why: Refusal::NotOurBus { address: refused }, .. }
                    if refused == address
            ),
            "{address}: {refusal:?}"
        );
    }
}

#[test]
fn a_refusal_of_an_address_names_it() {
    let refusal = Kwin::on("unix:path=/run/user/1000/bus").unwrap_err();
    assert_eq!(
        refusal.to_string(),
        "refusing to ask KWin: unix:path=/run/user/1000/bus is not a private bus of ours"
    );
}

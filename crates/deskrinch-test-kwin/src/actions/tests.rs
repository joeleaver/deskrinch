//! The actions handle's makers refuse, in the two forms carried over from
//! deskrinch-test-desktops: in-process (its makers are `Kwin`'s, whose
//! tables `kwin::tests` holds; here the handle is shown to be made only
//! through them), and in a child of this test binary outside any session,
//! where a maker that did not refuse would act on a bus address that leads
//! nowhere.

use std::process::{Command, Stdio};

use deskrinch_test_dbus::session::outside;
use deskrinch_test_session::bus;

use super::*;

#[test]
fn on_refuses_the_real_bus_and_takes_ours() {
    let refusal = KwinActions::on("unix:path=/run/user/1000/bus").unwrap_err();
    assert!(matches!(refusal, Error::NotOurBus { .. }), "{refusal:?}");
    let ours = bus::listen_address(&bus::socket_name(7, 1));
    KwinActions::on(&ours).unwrap();
}

/// Set by the outer tests for the inner ones, which do nothing without it.
const INNER: &str = "DESKRINCH_TEST_KWIN_ACTIONS_INNER";

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
fn setting_up_desktops_outside_a_session_is_refused_before_any_bus() {
    let said = refused_in_a_child("actions::tests::inner::sets_up_here");
    assert!(
        said.contains("refusing to ask KWin: not inside an isolated session"),
        "{said}"
    );
}

/// The address the child gives `on` is `outside`'s: a `unix:path=` address
/// with nothing listening, so that a maker that took it would reach
/// nothing.
#[test]
fn invoking_a_shortcut_on_an_address_not_ours_is_refused_before_any_bus() {
    let said = refused_in_a_child("actions::tests::inner::invokes_on_the_session_s_address");
    assert!(said.contains("is not a private bus of ours"), "{said}");
}

/// The public makers, run only by the outer tests above in their child;
/// each acts with the handle, should it have been made.
mod inner {
    use deskrinch_test_session::BUS_VARIABLE;

    use super::INNER;
    use crate::KwinActions;

    #[test]
    #[ignore = "run by its outer test, in a child outside any session"]
    fn sets_up_here() {
        if std::env::var_os(INNER).is_some()
            && let Err(why) = KwinActions::here().and_then(|kwin| kwin.set_up_virtual_desktops(4))
        {
            panic!("{why}");
        }
    }

    #[test]
    #[ignore = "run by its outer test, in a child outside any session"]
    fn invokes_on_the_session_s_address() {
        if std::env::var_os(INNER).is_some() {
            let address = std::env::var(BUS_VARIABLE).unwrap();
            if let Err(why) =
                KwinActions::on(&address).and_then(|kwin| kwin.invoke_kwin_shortcut("Overview"))
            {
                panic!("{why}");
            }
        }
    }
}

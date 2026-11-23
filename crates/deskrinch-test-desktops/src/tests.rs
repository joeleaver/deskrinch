//! The refusal outside a nested KWin session. A plain `cargo test` runs
//! these with `DESKRINCH_TEST_COMPOSITOR` unset. The check is tested for
//! sway and labwc as a function of the compositor, not by setting the
//! variable, which the other tests in the process would race.

use deskrinch_test_client::{Compositor, NotNested};

use super::{desktops, require_kwin, set_up};

#[test]
#[should_panic(expected = "this process runs in no nested session (not inside an isolated session")]
fn a_call_outside_a_nested_session_panics_before_any_bus() {
    let _listed = desktops();
}

#[test]
#[should_panic(
    expected = "the call would lay out, switch, add and remove the real session's desktops"
)]
fn setting_up_outside_a_nested_session_says_what_it_would_have_done() {
    let _first = set_up(4);
}

#[test]
#[should_panic(expected = "a nested sway, which has no KWin")]
fn sway_is_refused() {
    require_kwin(Ok(Compositor::Sway));
}

#[test]
#[should_panic(expected = "a nested labwc, which has no KWin")]
fn labwc_is_refused() {
    require_kwin(Ok(Compositor::Labwc));
}

#[test]
fn kwin_is_let_through() {
    require_kwin(Ok(Compositor::Kwin));
}

#[test]
#[should_panic(expected = "no nested session (the session's compositor is \"weston\"")]
fn an_unknown_compositor_is_refused_with_the_reason() {
    require_kwin(Err(NotNested::Unknown {
        compositor: "weston".to_owned(),
    }));
}

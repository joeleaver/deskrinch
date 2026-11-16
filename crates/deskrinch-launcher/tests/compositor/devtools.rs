//! rinch's DevTools keys do nothing in a shell component built without
//! `debug` (`deskrinch_component::app::devtools`): typed into the shown
//! launcher, F12 opens no DevTools window and Alt+I no inspect mode, and
//! Alt+I goes on to the search field as text, as rinch passes on a key it
//! does not consume.
//!
//! The sentinel is the application the field's query starts. After the
//! keys, `plain` is typed and Enter pressed; keys reach the launcher in the
//! order they are typed, so what it starts shows what the field held by
//! then. Alt+I reaching the field as an `i` makes the query `iplain`, which
//! only the test's own `Iplain` matches; an Alt+I rinch took for inspect
//! mode leaves `plain`, which starts `Plain Recorder`. That check is the one
//! that bites: with the constructor passing `true`, the test failed on it
//! under every compositor. Under KWin and sway the DevTools window F12 opens
//! takes the keyboard, at a point that varies, so the failure shows as the
//! query's results or the start not arriving, within the patience. rinch
//! writes no `[PERF] window … opened` line for the DevTools window, so the
//! watch cannot see it open, and nothing ends the wait sooner.
//!
//! As further evidence, read after the start is said: rinch's own lines
//! "DevTools: opened" and "Inspect mode: …" are not in the launcher's log.
//! They come on the same stream before the start is said, so they would be
//! there by then; but they are rinch's wording, and should rinch reword them
//! those two assertions would pass without showing anything.
//!
//! Under labwc only Alt+I is typed: the session binds F12 itself, to the
//! next workspace, for the workspace tests (`labwc_rc` in
//! `xtask/src/session/compositor/wlroots.rs` writes the `<keybind
//! key="F12">` into the session's `rc.xml`), so F12 never reaches the
//! launcher there.

use std::fs;

use deskrinch_test_client::Compositor;
use deskrinch_test_input::Key;
use deskrinch_test_wait::comes_true;

use crate::screen::{Shown, args, results_drawn, the_one_started};

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn the_devtools_keys_open_nothing_and_alt_i_is_typed() {
    let mut shown = Shown::start_with("", |dir| {
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=Iplain\nExec=/bin/sh {} Iplain\n",
            dir.join("record.sh").display()
        );
        fs::write(
            dir.join("data/applications/org.deskrinch.Iplain.desktop"),
            entry,
        )
        .unwrap();
    });
    if Compositor::current() != Compositor::Labwc {
        shown.input.press(Key::F12).unwrap();
    }
    shown
        .input
        .press_with(&[Key::LEFT_ALT], Key::typing('i').unwrap())
        .unwrap();
    shown.input.type_text("plain").unwrap();
    shown.see("the results of the query", results_drawn);
    shown.input.press(Key::ENTER).unwrap();
    assert_eq!(
        args(&the_one_started(&shown)),
        ["Iplain"],
        "Alt+I did not reach the field as an `i`"
    );
    assert!(
        comes_true(|| shown
            .launcher
            .said("recorded a start of org.deskrinch.Iplain.desktop")),
        "the launcher did not say it recorded the start; it said:\n{}",
        shown.launcher.log()
    );
    for said in ["DevTools: opened", "Inspect mode"] {
        assert!(
            !shown.launcher.said(said),
            "rinch said {said:?}; the launcher said:\n{}",
            shown.launcher.log()
        );
    }
}

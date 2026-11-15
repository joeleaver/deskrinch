//! The hidden launcher has rinch prepare its window for the next show
//! (`deskrinch_launcher::Preparation`): when what that show draws changes,
//! and at every hide, each request said in its log (`deskrinch_launcher::preparing`) and
//! followed by rinch's prepared frame. It presents nothing while hidden, and
//! an idle hidden launcher prepares nothing more. That the first show then
//! draws the right content is [`crate::screen`]'s and [`crate::frequent`]'s
//! tests, which summon a hidden launcher.

use deskrinch_launcher::Change::{Everything, Hidden, Icons, Results};

use crate::frequent::write_record;
use crate::screen::Shown;
use crate::support::{client, stays};

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_hidden_launcher_prepares_for_its_results_and_its_icons() {
    let hidden = Shown::hidden("", |dir| write_record(dir, &["Plain", "Other"]));
    let launcher = &hidden.launcher;
    // The results and the icons may land before the first request, which
    // then names everything.
    if let Err(why) = launcher.wait_prepared_for(&[Results, Everything]) {
        panic!("the results: {why}");
    }
    if let Err(why) = launcher.wait_prepared_for(&[Icons, Everything]) {
        panic!("the icons: {why}");
    }
    assert_eq!(launcher.frames(), 0, "a hidden launcher presented");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_hide_prepares_for_the_next_show() {
    let mut shown = Shown::hidden("", |dir| write_record(dir, &["Plain"]));
    // Prepared once while hidden first: a launcher shown before its first
    // preparation says at its first hide that nothing was prepared before.
    if let Err(why) = shown.launcher.wait_prepared_for(&[Everything]) {
        panic!("at start: {why}");
    }
    shown.show_again();
    assert!(client(&["hide"]).success());
    if let Err(why) = shown.launcher.wait_prepared_for(&[Hidden]) {
        panic!("the hide: {why}");
    }
}

/// A launcher shown before it ever prepared (made shown, as here, or
/// summoned before its first preparation) prepares at its first hide, which
/// says that nothing was prepared before.
#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_launcher_made_shown_prepares_at_its_first_hide() {
    let shown = Shown::start("");
    assert_eq!(shown.launcher.prepared(), 0, "a shown launcher prepared");
    assert!(client(&["hide"]).success());
    if let Err(why) = shown.launcher.wait_prepared_for(&[Everything]) {
        panic!("the first hide: {why}");
    }
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_idle_hidden_launcher_presents_and_prepares_nothing() {
    let hidden = Shown::hidden("", |_| {});
    let launcher = &hidden.launcher;
    if let Err(why) = launcher.wait_prepared_for(&[Icons, Everything]) {
        panic!("the icons: {why}");
    }
    let prepared = launcher.prepared();
    assert!(
        stays(|| launcher.prepared() == prepared && launcher.frames() == 0),
        "an idle hidden launcher prepared or presented; its last lines:\n{}",
        launcher.log()
    );
}

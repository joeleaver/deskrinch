//! The hidden launcher has rinch prepare its window for the next show
//! (`deskrinch_launcher::Preparation`): when what that show draws changes,
//! and at every hide, each request said in its log (`deskrinch_launcher::preparing`) and
//! followed by rinch's prepared frame. It presents nothing while hidden, and
//! an idle hidden launcher prepares nothing more. That the first show then
//! draws the right content is [`crate::screen`]'s and [`crate::frequent`]'s
//! tests, which summon a hidden launcher.
//!
//! The order of the requests (a prepare after each hide, none while shown)
//! is also the binary's `root::tests`', in rinch's test harness. Those say
//! the launcher asks; these say a real runtime honours the request on a
//! real layer surface, with a prepared frame and nothing presented.

use std::fs;
use std::path::Path;

use deskrinch_launcher::Change::{Everything, Hidden, Icons, Results};

use crate::frequent::write_record;
use crate::screen::Shown;
use crate::support::client;

/// An icon theme installed in the test's data directory, which nothing
/// chooses until `[general]` `icon-theme` names it.
const THEME: &str = "deskrinch-test";

/// Installs [`THEME`] under `dir`'s data directory: an `index.theme` with
/// no directories, which is a theme all the same.
fn install_theme(dir: &Path) {
    let theme = dir.join("data/icons").join(THEME);
    fs::create_dir_all(&theme).unwrap();
    fs::write(
        theme.join("index.theme"),
        "[Icon Theme]\nName=Test\nDirectories=\n",
    )
    .unwrap();
}

/// Writes the test's configuration file whole, as an editor saving it
/// would: a new file renamed over the old.
fn write_config(dir: &Path, text: &str) {
    let path = dir.join("config/deskrinch/config.toml");
    let next = path.with_extension("toml.next");
    fs::write(&next, text).unwrap();
    fs::rename(&next, &path).unwrap();
}

/// Each change is made by the test while the launcher is hidden and
/// settled, and the request for it must come after the change and name it:
/// the start's own request (naming everything) cannot answer for either.
#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_hidden_launcher_prepares_for_its_results_and_its_icons() {
    let hidden = Shown::hidden("", |dir| {
        write_record(dir, &["Plain", "Other"]);
        install_theme(dir);
    });
    let launcher = &hidden.launcher;
    if let Err(why) = launcher.wait_settled_hidden() {
        panic!("at start: {why}");
    }
    // The results: with the record turned off, an empty query shows none.
    let mark = launcher.mark();
    write_config(hidden.dir(), "[general]\nremember-starts = false\n");
    if let Err(why) = launcher.wait_prepared_for_since(mark, &[Results]) {
        panic!("the results: {why}");
    }
    if let Err(why) = launcher.wait_settled_hidden() {
        panic!("after the results: {why}");
    }
    // The icons: another icon theme, so another index.
    let mark = launcher.mark();
    write_config(
        hidden.dir(),
        &format!("[general]\nremember-starts = false\nicon-theme = \"{THEME}\"\n"),
    );
    if let Err(why) = launcher.wait_prepared_for_since(mark, &[Icons]) {
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

/// Once its start is prepared (whenever the icon index lands), a hidden
/// launcher stops preparing, and presents nothing throughout.
#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_idle_hidden_launcher_presents_and_prepares_nothing() {
    let hidden = Shown::hidden("", |_| {});
    let launcher = &hidden.launcher;
    if let Err(why) = launcher.wait_prepared_for(&[Everything]) {
        panic!("at start: {why}");
    }
    if let Err(why) = launcher.wait_settled_hidden() {
        panic!("idle: {why}");
    }
}

//! When the launcher prepares, on made-up content, and what it says.

use std::path::Path;

use deskrinch_desktop_entries::DesktopId;
use deskrinch_icons::SearchPath;

use super::Change::{Everything, Hidden, Icons, Look, Results};
use super::*;

/// No change: nothing to prepare.
const NOTHING: [Change; 0] = [];

fn row(name: &str) -> Row {
    Row {
        id: DesktopId::new(format!("org.example.{name}.desktop")).unwrap(),
        name: name.to_owned(),
        icon: None,
    }
}

/// A new index, unlike any other: no base directory exists, so nothing is
/// read.
fn index() -> Arc<Index> {
    let none = SearchPath::from_dirs(vec![Path::new("/nonexistent").to_owned()]);
    Arc::new(Index::load(&none, "t"))
}

fn drawn(rows: &[&str], icons: Option<&Arc<Index>>, look: Scheme) -> Drawn {
    Drawn {
        rows: rows.iter().map(|name| row(name)).collect(),
        icons: icons.cloned(),
        look,
    }
}

#[test]
fn prepares_at_start_when_hidden_and_not_when_shown() {
    let empty = drawn(&[], None, Scheme::Dark);
    assert_eq!(
        Preparation::default().wants(empty.clone(), false),
        [Everything]
    );
    assert_eq!(Preparation::default().wants(empty, true), NOTHING);
}

#[test]
fn prepares_again_when_the_results_the_icons_or_the_look_change_while_hidden() {
    let mut preparation = Preparation::default();
    let first = index();
    let empty = drawn(&[], None, Scheme::Dark);
    assert_eq!(preparation.wants(empty.clone(), false), [Everything]);
    assert_eq!(preparation.wants(empty, false), NOTHING);
    // The applications and the record of starts landed: the results.
    let rows = drawn(&["a", "b"], None, Scheme::Dark);
    assert_eq!(preparation.wants(rows, false), [Results]);
    // The icon theme's index landed.
    let icons = drawn(&["a", "b"], Some(&first), Scheme::Dark);
    assert_eq!(preparation.wants(icons.clone(), false), [Icons]);
    assert_eq!(preparation.wants(icons, false), NOTHING);
    // A new index, though equal in content, is new.
    let rebuilt = index();
    let again = drawn(&["a", "b"], Some(&rebuilt), Scheme::Dark);
    assert_eq!(preparation.wants(again, false), [Icons]);
    // The look changed.
    let light = drawn(&["a", "b"], Some(&rebuilt), Scheme::Light);
    assert_eq!(preparation.wants(light, false), [Look]);
    // Two at once are both named.
    let both = drawn(&["b"], Some(&first), Scheme::Light);
    assert_eq!(preparation.wants(both, false), [Results, Icons]);
}

#[test]
fn every_hide_prepares_naming_what_changed_while_shown() {
    let mut preparation = Preparation::default();
    let first = drawn(&["a"], None, Scheme::Dark);
    assert_eq!(preparation.wants(first.clone(), false), [Everything]);
    // Shown: its frames are its preparation, whatever changes.
    assert_eq!(preparation.wants(first.clone(), true), NOTHING);
    let light = drawn(&["b"], None, Scheme::Light);
    assert_eq!(preparation.wants(light.clone(), true), NOTHING);
    // Hidden: the hide prepares, with what changed since the last.
    assert_eq!(
        preparation.wants(light.clone(), false),
        [Hidden, Results, Look]
    );
    assert_eq!(preparation.wants(light.clone(), false), NOTHING);
    // A hide that changed nothing prepares too: the last painted frame
    // was the shown one's.
    assert_eq!(preparation.wants(light.clone(), true), NOTHING);
    assert_eq!(preparation.wants(light, false), [Hidden]);
}

#[test]
fn a_launcher_made_shown_prepares_everything_at_its_first_hide() {
    let mut preparation = Preparation::default();
    let rows = drawn(&["a"], None, Scheme::Dark);
    assert_eq!(preparation.wants(rows.clone(), true), NOTHING);
    assert_eq!(preparation.wants(rows, false), [Everything]);
}

#[test]
fn drawn_is_equal_when_nothing_a_show_draws_differs() {
    let icons = index();
    let a = drawn(&["a"], Some(&icons), Scheme::Dark);
    assert_eq!(a, a.clone());
    assert_ne!(a, drawn(&["a"], Some(&index()), Scheme::Dark));
    assert_ne!(a, drawn(&["a"], None, Scheme::Dark));
    assert_ne!(a, drawn(&[], Some(&icons), Scheme::Dark));
}

#[test]
fn the_line_says_why() {
    assert_eq!(
        preparing(&[Everything]),
        "preparing the hidden window for its next show: nothing was prepared before"
    );
    assert_eq!(
        preparing(&[Hidden, Results, Icons]),
        format!("{PREPARING} it was hidden; the results changed; the icons changed")
    );
}

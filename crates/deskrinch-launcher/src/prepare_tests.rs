//! When the launcher prepares, on made-up content.

use std::path::Path;

use deskrinch_desktop_entries::DesktopId;
use deskrinch_icons::SearchPath;

use super::*;

/// No change: nothing to prepare.
const NOTHING: [&str; 0] = [];
/// The first preparation's.
const EVERYTHING: [&str; 1] = ["everything (not prepared before)"];

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
        EVERYTHING
    );
    assert_eq!(Preparation::default().wants(empty, true), NOTHING);
}

#[test]
fn prepares_again_when_the_results_the_icons_or_the_look_change_while_hidden() {
    let mut preparation = Preparation::default();
    let first = index();
    let empty = drawn(&[], None, Scheme::Dark);
    assert_eq!(preparation.wants(empty.clone(), false), EVERYTHING);
    assert_eq!(preparation.wants(empty, false), NOTHING);
    // The applications and the record of starts landed: the results.
    let rows = drawn(&["a", "b"], None, Scheme::Dark);
    assert_eq!(preparation.wants(rows, false), ["the results"]);
    // The icon theme's index landed.
    let icons = drawn(&["a", "b"], Some(&first), Scheme::Dark);
    assert_eq!(preparation.wants(icons.clone(), false), ["the icons"]);
    assert_eq!(preparation.wants(icons, false), NOTHING);
    // A new index, though equal in content, is new.
    let rebuilt = index();
    let again = drawn(&["a", "b"], Some(&rebuilt), Scheme::Dark);
    assert_eq!(preparation.wants(again, false), ["the icons"]);
    // The look changed.
    let light = drawn(&["a", "b"], Some(&rebuilt), Scheme::Light);
    assert_eq!(preparation.wants(light, false), ["the look"]);
    // Two at once are both named.
    let both = drawn(&["b"], Some(&first), Scheme::Light);
    assert_eq!(preparation.wants(both, false), ["the results", "the icons"]);
}

#[test]
fn a_change_while_shown_is_prepared_with_the_next_change_while_hidden() {
    let mut preparation = Preparation::default();
    assert_eq!(
        preparation.wants(drawn(&["a"], None, Scheme::Dark), false),
        EVERYTHING
    );
    // Shown: its frames are its preparation, whatever changes.
    let more = drawn(&["a", "b"], None, Scheme::Dark);
    assert_eq!(preparation.wants(more, true), NOTHING);
    let light = drawn(&["b"], None, Scheme::Light);
    assert_eq!(preparation.wants(light, true), NOTHING);
    // Hidden again, the next change prepares everything since the last.
    let icons = index();
    assert_eq!(
        preparation.wants(drawn(&["b"], Some(&icons), Scheme::Light), false),
        ["the results", "the icons", "the look"]
    );
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

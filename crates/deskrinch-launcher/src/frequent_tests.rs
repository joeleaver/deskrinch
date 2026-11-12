//! The empty query: the applications used most ([`frequent`]), and when the
//! model shows them.

use super::tests::{app, id, ids, selected};
use super::*;

const T0: u64 = 1_800_000_000;

fn installed() -> Vec<DesktopEntry> {
    ["a", "b", "c", "d", "e", "f", "g"]
        .iter()
        .map(|name| app(name, &name.to_uppercase(), ""))
        .collect()
}

/// A record in which each of `names` was started more than the next.
fn record(names: &[&str]) -> Record {
    let mut record = Record::new();
    for (n, name) in names.iter().enumerate() {
        for _ in n..names.len() {
            record.record(&id(name), T0);
        }
    }
    record
}

fn row_ids(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|row| row.id.app_id()).collect()
}

/// A model whose empty query shows `names`, best first.
fn showing(names: &[&str]) -> Model {
    let mut model = Model::new();
    model.set_frequent(frequent(&record(names), &installed(), &[]));
    model.clear();
    model
}

#[test]
fn the_best_installed_are_taken_in_the_records_order() {
    let rows = frequent(&record(&["c", "gone", "a", "f"]), &installed(), &[]);
    assert_eq!(row_ids(&rows), ["c", "a", "f"]);
    assert_eq!(rows[0].name, "C");
}

#[test]
fn only_the_first_few_are_taken() {
    let rows = frequent(&record(&["g", "f", "e", "d", "c", "b", "a"]), &installed(), &[]);
    assert_eq!(rows.len(), FREQUENT);
    assert_eq!(row_ids(&rows), ["g", "f", "e", "d", "c"]);
}

#[test]
fn no_record_shows_nothing() {
    assert!(frequent(&Record::new(), &installed(), &[]).is_empty());
    let model = showing(&[]);
    assert_eq!(model, Model::new());
}

#[test]
fn rows_are_in_the_users_language() {
    let entries = [app("a", "Files", "Name[de]=Dateien\nIcon=folder\n")];
    let locales = [Locale::parse("de").unwrap()];
    let rows = frequent(&record(&["a"]), &entries, &locales);
    assert_eq!(rows[0].name, "Dateien");
    assert_eq!(rows[0].icon, Some(Icon::Name("folder".into())));
}

#[test]
fn an_empty_query_shows_them_with_the_first_selected_and_enter_starts_it() {
    let mut model = showing(&["b", "a"]);
    assert_eq!(model.query(), "");
    assert_eq!(ids(&model), ["b", "a"]);
    assert_eq!(selected(&model), Some("b"));
    model.input(Input::Down);
    assert_eq!(selected(&model), Some("a"));
    assert_eq!(model.input(Input::Enter), Action::Launch(id("a")));
}

#[test]
fn typing_searches_and_emptying_the_query_shows_them_again() {
    let entries = installed();
    let mut model = showing(&["b", "a"]);
    model.set_query("c", &entries, &[]);
    assert_eq!(ids(&model), ["c"]);
    model.set_query("", &entries, &[]);
    assert_eq!(ids(&model), ["b", "a"]);
    assert_eq!(selected(&model), Some("b"));
}

#[test]
fn new_ones_wait_for_the_next_clear_and_never_replace_what_is_shown() {
    let entries = installed();
    let mut model = showing(&["b", "a"]);
    model.input(Input::Down);
    model.set_frequent(frequent(&record(&["c"]), &entries, &[]));
    assert_eq!(ids(&model), ["b", "a"], "what is shown stays");
    assert_eq!(selected(&model), Some("a"));
    model.clear();
    assert_eq!(ids(&model), ["c"]);
    // Emptying the query also shows the newest.
    model.set_frequent(frequent(&record(&["d"]), &entries, &[]));
    model.set_query("x", &entries, &[]);
    model.set_query("", &entries, &[]);
    assert_eq!(ids(&model), ["d"]);
}

#[test]
fn an_application_removed_while_shown_leaves_them() {
    let mut model = showing(&["b", "a", "c"]);
    model.input(Input::Down);
    let left: Vec<DesktopEntry> = installed()
        .into_iter()
        .filter(|entry| entry.id != id("b"))
        .collect();
    model.refresh(&left, &[]);
    assert_eq!(ids(&model), ["a", "c"]);
    assert_eq!(selected(&model), Some("a"), "the selection stays on it");
    // A refresh adds none of the others.
    model.refresh(&installed(), &[]);
    assert_eq!(ids(&model), ["a", "c"]);
}

#[test]
fn one_start_per_show_holds_for_them_too() {
    let mut model = showing(&["b", "a"]);
    assert_eq!(model.input(Input::Enter), Action::Launch(id("b")));
    assert_eq!(model.input(Input::Enter), Action::Stay);
    model.clear();
    assert_eq!(model.input(Input::Enter), Action::Launch(id("b")));
}

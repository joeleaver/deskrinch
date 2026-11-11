use super::*;

/// An application entry with `id`, `name` and any further keys in `extra`.
fn app(id: &str, name: &str, extra: &str) -> DesktopEntry {
    let text = format!("[Desktop Entry]\nType=Application\nName={name}\nExec=run-{id}\n{extra}");
    let id = DesktopId::new(format!("{id}.desktop")).unwrap();
    DesktopEntry::parse(id, "/x".into(), &text).unwrap()
}

fn terminals() -> Vec<DesktopEntry> {
    vec![
        app("ultraterm", "Ultraterm", ""),
        app("terminal", "Terminal", "Icon=utilities-terminal\n"),
        app("term", "Term", ""),
        app("files", "Files", ""),
    ]
}

/// The rows' app ids, in order.
fn ids(model: &Model) -> Vec<&str> {
    model.rows().iter().map(|row| row.id.app_id()).collect()
}

fn selected(model: &Model) -> Option<&str> {
    model.selected_row().map(|row| row.id.app_id())
}

fn id(name: &str) -> DesktopId {
    DesktopId::new(format!("{name}.desktop")).unwrap()
}

#[test]
fn starts_empty() {
    let model = Model::new();
    assert_eq!(model.query(), "");
    assert_eq!(model.rows(), Vec::<Row>::new());
    assert_eq!(model.selected(), None);
}

#[test]
fn typing_ranks_by_the_search_and_selects_the_first() {
    let entries = terminals();
    let mut model = Model::new();
    model.set_query("term", &entries, &[]);
    assert_eq!(model.query(), "term");
    assert_eq!(ids(&model), ["term", "terminal", "ultraterm"]);
    assert_eq!(model.selected(), Some(0));
    // Typing on narrows the results, and the first is selected again.
    model.input(Input::Down);
    model.set_query("termi", &entries, &[]);
    assert_eq!(ids(&model), ["terminal"]);
    assert_eq!(selected(&model), Some("terminal"));
}

#[test]
fn rows_carry_the_translated_name_and_icon() {
    let entries = [app(
        "files",
        "Files",
        "Name[de]=Dateien\nIcon=folder\nIcon[de]=ordner\n",
    )];
    let german = [Locale::parse("de_DE").unwrap()];
    let mut model = Model::new();
    model.set_query("dat", &entries, &german);
    let row = &model.rows()[0];
    assert_eq!(row.name, "Dateien");
    assert_eq!(row.icon, Some(Icon::Name("ordner".into())));
    model.set_query("files", &entries, &[]);
    assert_eq!(model.rows()[0].name, "Files");
    assert_eq!(model.rows()[0].icon, Some(Icon::Name("folder".into())));
}

#[test]
fn a_query_that_matches_nothing_has_no_selection() {
    let entries = terminals();
    let mut model = Model::new();
    for query in ["", "   ", "zzz"] {
        model.set_query(query, &entries, &[]);
        assert_eq!(model.rows(), Vec::<Row>::new(), "{query:?}");
        assert_eq!(model.selected(), None);
        assert_eq!(model.input(Input::Down), Action::Stay);
        assert_eq!(model.input(Input::Up), Action::Stay);
        assert_eq!(model.selected(), None);
        assert_eq!(model.input(Input::Enter), Action::Stay);
    }
}

#[test]
fn up_and_down_move_and_stop_at_the_ends() {
    let entries = terminals();
    let mut model = Model::new();
    model.set_query("term", &entries, &[]);
    assert_eq!(model.input(Input::Up), Action::Stay);
    assert_eq!(selected(&model), Some("term"));
    model.input(Input::Down);
    assert_eq!(selected(&model), Some("terminal"));
    model.input(Input::Down);
    model.input(Input::Down);
    assert_eq!(selected(&model), Some("ultraterm"));
    model.input(Input::Up);
    assert_eq!(selected(&model), Some("terminal"));
}

#[test]
fn enter_starts_the_selected_application() {
    let entries = terminals();
    let mut model = Model::new();
    model.set_query("term", &entries, &[]);
    model.input(Input::Down);
    assert_eq!(model.input(Input::Enter), Action::Launch(id("terminal")));
}

#[test]
fn escape_hides() {
    let entries = terminals();
    let mut model = Model::new();
    model.set_query("term", &entries, &[]);
    assert_eq!(model.input(Input::Escape), Action::Hide);
}

#[test]
fn losing_the_keyboard_hides_whatever_the_query() {
    let entries = terminals();
    for query in ["", "zzz", "term"] {
        let mut model = Model::new();
        model.set_query(query, &entries, &[]);
        model.input(Input::Down);
        assert_eq!(model.input(Input::KeyboardLost), Action::Hide, "{query:?}");
    }
}

#[test]
fn an_installed_application_keeps_the_selection_where_it_was() {
    let mut entries = terminals();
    let mut model = Model::new();
    model.set_query("term", &entries, &[]);
    model.input(Input::Down);
    assert_eq!(selected(&model), Some("terminal"));
    // Ranks first, above the selected one.
    entries.push(app("aterm", "Term", ""));
    model.refresh(&entries, &[]);
    assert_eq!(ids(&model), ["aterm", "term", "terminal", "ultraterm"]);
    assert_eq!(selected(&model), Some("terminal"));
    assert_eq!(model.query(), "term");
}

#[test]
fn a_removed_selection_falls_back_to_the_first() {
    let mut entries = terminals();
    let mut model = Model::new();
    model.set_query("term", &entries, &[]);
    model.input(Input::Down);
    entries.retain(|entry| entry.id != id("terminal"));
    model.refresh(&entries, &[]);
    assert_eq!(ids(&model), ["term", "ultraterm"]);
    assert_eq!(selected(&model), Some("term"));
    // With every match removed, nothing is selected.
    entries.clear();
    model.refresh(&entries, &[]);
    assert_eq!(model.selected(), None);
}

#[test]
fn clearing_empties_it_for_the_next_show() {
    let entries = terminals();
    let mut model = Model::new();
    model.set_query("term", &entries, &[]);
    model.input(Input::Down);
    model.clear();
    assert_eq!(model, Model::new());
}

#[test]
fn draws_the_rows_that_keep_the_selection_in_sight() {
    let entries: Vec<DesktopEntry> = (0..10)
        .map(|n| app(&format!("app{n}"), &format!("App {n}"), ""))
        .collect();
    let mut model = Model::new();
    model.set_query("app", &entries, &[]);
    assert_eq!(model.visible(4), 0..4);
    for _ in 0..3 {
        model.input(Input::Down);
    }
    assert_eq!(model.visible(4), 0..4, "the fourth row is still in sight");
    model.input(Input::Down);
    assert_eq!(model.visible(4), 1..5);
    assert_eq!(model.visible(20), 0..10, "more room than rows");
    model.set_query("zzz", &entries, &[]);
    assert_eq!(model.visible(4), 0..0);
}

/// A model searched for "term", whose results include `term` and
/// `terminal`.
fn clickable() -> Model {
    let entries = terminals();
    let mut model = Model::new();
    model.set_query("term", &entries, &[]);
    assert!(ids(&model).contains(&"terminal") && ids(&model).contains(&"term"));
    model
}

#[test]
fn a_click_on_a_result_starts_it_without_moving_the_selection() {
    let mut model = clickable();
    let before = model.selected();
    assert_eq!(
        model.input(Input::Press(Some(id("terminal")))),
        Action::Stay
    );
    assert_eq!(model.selected(), before, "no hover or press selection");
    assert_eq!(
        model.input(Input::Release(Some(id("terminal")))),
        Action::Launch(id("terminal"))
    );
}

#[test]
fn a_press_and_a_release_on_different_results_start_nothing() {
    let mut model = clickable();
    model.input(Input::Press(Some(id("terminal"))));
    assert_eq!(model.input(Input::Release(Some(id("term")))), Action::Stay);
    // The press is spent: a release alone is no click.
    assert_eq!(model.input(Input::Release(Some(id("term")))), Action::Stay);
}

#[test]
fn a_press_or_a_release_off_the_results_starts_nothing() {
    let mut model = clickable();
    model.input(Input::Press(Some(id("terminal"))));
    assert_eq!(model.input(Input::Release(None)), Action::Stay);
    assert_eq!(
        model.input(Input::Release(Some(id("terminal")))),
        Action::Stay
    );

    model.input(Input::Press(Some(id("terminal"))));
    model.input(Input::Press(None));
    assert_eq!(
        model.input(Input::Release(Some(id("terminal")))),
        Action::Stay,
        "a later press elsewhere replaced it"
    );
}

#[test]
fn a_click_on_a_result_that_went_away_meanwhile_starts_nothing() {
    let mut model = clickable();
    model.input(Input::Press(Some(id("terminal"))));
    model.set_query("files", &terminals(), &[]);
    assert_eq!(
        model.input(Input::Release(Some(id("terminal")))),
        Action::Stay
    );
}

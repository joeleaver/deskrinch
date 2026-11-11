//! The open menus in the model, apart from the bus: opening and closing,
//! pacing, which entry is which across readings, and stale answers.

use std::time::Duration;

use deskrinch_untrusted::Image;
use zbus::wire::OwnedObjectPath;

use super::menus::MenuDone;
use super::*;
use crate::item::Icon;
use crate::menu::{EntryId, EntryKind, Label, MenuEntry, Toggle};
use crate::parse;
use crate::wire::Properties;

const SERVICE: &str = "org.kde.StatusNotifierItem-1-1";
const MENU: &str = "/MenuBar";

fn t(ms: u64) -> Instant {
    use std::sync::OnceLock;
    static ZERO: OnceLock<Instant> = OnceLock::new();
    *ZERO.get_or_init(Instant::now) + Duration::from_millis(ms)
}

fn interval() -> Duration {
    Limits::default().min_read_interval
}

/// The item `id`, with its menu at `menu`.
fn item(id: &ItemId, menu: Option<&str>) -> Outcome {
    let properties = Properties {
        title: Some("A".into()),
        menu: menu.map(|m| OwnedObjectPath::try_from(m).unwrap()),
        ..Properties::default()
    };
    Outcome::Read(Box::new(parse::item(
        id.clone(),
        properties,
        &Limits::default(),
    )))
}

/// A model with one item, owned by `:1.5`, read with its menu at `MENU`.
fn model_with(limits: Limits) -> (Model, ItemId) {
    let mut model = Model::new(limits);
    let target = Target {
        service: SERVICE.into(),
        path: "/StatusNotifierItem".into(),
    };
    let id = model.register(target, ":1.5", t(0)).unwrap().unwrap().id;
    model.read_done(&id, ":1.5", item(&id, Some(MENU)), t(0));
    (model, id)
}

fn model() -> (Model, ItemId) {
    model_with(Limits::default())
}

/// An entry as parsed, numbered `id` by the application.
fn entry(id: i32, label: &str) -> MenuEntry {
    MenuEntry {
        id: EntryId { id, since: 0 },
        kind: EntryKind::Standard,
        label: Label {
            text: label.into(),
            mnemonic: None,
        },
        enabled: true,
        icon: Icon::default(),
        toggle: Toggle::None,
        shortcuts: Vec::new(),
        disposition: crate::menu::Disposition::Normal,
        submenu: None,
    }
}

fn layout(id: &ItemId, entries: Vec<MenuEntry>) -> MenuOutcome {
    MenuOutcome::Read(Box::new(Menu {
        item: id.clone(),
        entries,
        text_direction: crate::menu::TextDirection::LeftToRight,
        icon_theme_path: Vec::new(),
    }))
}

/// Opens the menu at `t(10)` and starts its first read; its source.
fn open(model: &mut Model, id: &ItemId) -> MenuSource {
    let (_, due) = model.open_menu(id, t(10)).unwrap().unwrap();
    assert_eq!(due.at, t(10));
    model.menu_source(id).unwrap()
}

/// The entries of the stored menu.
fn stored(model: &Model, id: &ItemId) -> Vec<MenuEntry> {
    model.menu(id).unwrap().entries.clone()
}

#[test]
fn only_a_read_item_with_a_menu_opens_one_and_only_once() {
    let mut model = Model::new(Limits::default());
    let target = Target {
        service: SERVICE.into(),
        path: "/StatusNotifierItem".into(),
    };
    let id = model.register(target, ":1.5", t(0)).unwrap().unwrap().id;
    assert_eq!(model.open_menu(&id, t(0)), Err(MenuRefusal::NotFound));
    model.read_done(&id, ":1.5", item(&id, None), t(0));
    assert_eq!(model.open_menu(&id, t(0)), Err(MenuRefusal::NoMenu));
    let (mut model, id) = self::model();
    let (target, _) = model.open_menu(&id, t(10)).unwrap().unwrap();
    assert_eq!(
        target,
        MenuTarget {
            service: SERVICE.into(),
            item_path: "/StatusNotifierItem".into(),
            path: MENU.into(),
        }
    );
    assert_eq!(model.open_menu(&id, t(20)), Ok(None));
}

#[test]
fn the_first_read_of_an_opening_says_about_to_show_and_no_other() {
    let (mut model, id) = model();
    let first = open(&mut model, &id);
    assert!(first.about_to_show);
    assert_eq!((first.owner.as_str(), first.path.as_str()), (":1.5", MENU));
    model.menu_read_done(&first, layout(&id, vec![entry(1, "Open")]), t(20));
    let next = model.menu_signalled(":1.5", MENU, t(30)).pop().unwrap();
    assert_eq!(next.at, t(10) + interval());
    assert!(!model.menu_source(&id).unwrap().about_to_show);
}

#[test]
fn a_read_is_reported_when_it_differs() {
    let (mut model, id) = model();
    let source = open(&mut model, &id);
    let done = model.menu_read_done(&source, layout(&id, vec![entry(1, "Open")]), t(20));
    assert_eq!(done.change, Some(Change::Menu(model.menu(&id).unwrap())));
    // The same again says nothing.
    model.menu_signalled(":1.5", MENU, t(30));
    let source = model.menu_source(&id).unwrap();
    let done = model.menu_read_done(&source, layout(&id, vec![entry(1, "Open")]), t(200));
    assert_eq!(done.change, None);
}

#[test]
fn an_entry_whose_number_now_names_another_is_not_the_one_the_caller_saw() {
    let (mut model, id) = model();
    let source = open(&mut model, &id);
    model.menu_read_done(
        &source,
        layout(
            &id,
            vec![entry(1, "Open"), entry(2, "Uptime 1s"), entry(3, "Quit")],
        ),
        t(20),
    );
    let [open, uptime, quit] = [0, 1, 2].map(|i| stored(&model, &id)[i].id);
    let any = |_: &MenuEntry| true;
    assert_eq!(model.entry_target(&id, open, any).unwrap().path, MENU);

    // Positional numbering: "Open" is gone, and its number is "Quit"'s now;
    // "Uptime"'s label changed.
    model.menu_signalled(":1.5", MENU, t(30));
    let source = model.menu_source(&id).unwrap();
    model.menu_read_done(
        &source,
        layout(&id, vec![entry(1, "Quit"), entry(2, "Uptime 2s")]),
        t(200),
    );
    assert_eq!(
        model.entry_target(&id, open, any),
        Err(MenuRefusal::NoEntry)
    );
    assert_eq!(
        model.entry_target(&id, uptime, any),
        Err(MenuRefusal::NoEntry)
    );
    assert_eq!(
        model.entry_target(&id, quit, any),
        Err(MenuRefusal::NoEntry)
    );
    let now = stored(&model, &id);
    assert_eq!(model.entry_target(&id, now[0].id, any).unwrap().path, MENU);

    // Only its other properties changing leaves an entry the same.
    let mut disabled = entry(1, "Quit");
    disabled.enabled = false;
    model.menu_signalled(":1.5", MENU, t(300));
    let source = model.menu_source(&id).unwrap();
    model.menu_read_done(
        &source,
        layout(&id, vec![disabled, entry(2, "Uptime 2s")]),
        t(400),
    );
    assert_eq!(stored(&model, &id)[0].id, now[0].id);
    assert_eq!(stored(&model, &id)[1].id, now[1].id);
    // Refused when the caller asks what it cannot do.
    let enabled = |e: &MenuEntry| e.enabled;
    assert_eq!(
        model.entry_target(&id, now[0].id, enabled),
        Err(MenuRefusal::NoEntry)
    );
}

#[test]
fn ids_from_an_earlier_opening_name_nothing() {
    let (mut model, id) = model();
    let source = open(&mut model, &id);
    model.menu_read_done(&source, layout(&id, vec![entry(1, "Open")]), t(20));
    let before = stored(&model, &id)[0].id;
    model.close_menu(&id).unwrap();
    assert_eq!(model.menu(&id), None);
    let source = open(&mut model, &id);
    model.menu_read_done(&source, layout(&id, vec![entry(1, "Open")]), t(300));
    assert_eq!(
        model.entry_target(&id, before, |_| true),
        Err(MenuRefusal::NoEntry)
    );
}

#[test]
fn a_first_read_that_fails_closes_the_menu_and_a_later_one_keeps_it() {
    let (mut model, id) = model();
    let source = open(&mut model, &id);
    let done = model.menu_read_done(&source, MenuOutcome::Failed(MenuFailure::TimedOut), t(20));
    assert_eq!(
        done,
        MenuDone {
            change: Some(Change::MenuFailed(id.clone(), MenuFailure::TimedOut)),
            next: None,
        }
    );
    assert_eq!(model.menu_source(&id), None);

    let source = open(&mut model, &id);
    model.menu_read_done(&source, layout(&id, vec![entry(1, "Open")]), t(30));
    model.menu_signalled(":1.5", MENU, t(40));
    let source = model.menu_source(&id).unwrap();
    let done = model.menu_read_done(&source, MenuOutcome::Failed(MenuFailure::OverBound), t(200));
    assert_eq!(done.change, None);
    assert_eq!(stored(&model, &id)[0].label.text, "Open");
}

#[test]
fn an_answer_from_a_replaced_owner_or_moved_menu_is_stale() {
    let (mut model, id) = model();
    let source = open(&mut model, &id);
    // The name moves while the first read is in flight: one more read.
    let moved = model.owner_changed(SERVICE, Some(":1.9"), t(15));
    assert_eq!(moved.menu_reads, Vec::<MenuDue>::new());
    // :1.5 answers anyway, even with a failure: nothing is kept or said.
    let done = model.menu_read_done(&source, layout(&id, vec![entry(1, "old")]), t(20));
    assert_eq!(done.change, None);
    assert_eq!(model.menu(&id), None);
    let next = done.next.unwrap();
    let source = model.menu_source(&next.id).unwrap();
    assert_eq!(source.owner, ":1.9");
    let done = model.menu_read_done(&source, layout(&id, vec![entry(1, "new")]), next.at);
    assert!(matches!(done.change, Some(Change::Menu(_))));
}

#[test]
fn a_menu_that_moves_is_read_where_it_is_and_one_that_goes_closes() {
    let (mut model, id) = model();
    let source = open(&mut model, &id);
    model.menu_read_done(&source, layout(&id, vec![entry(1, "Open")]), t(20));
    // The item is read again with its menu elsewhere.
    model.signalled(":1.5", "/StatusNotifierItem", t(30));
    let done = model.read_done(&id, ":1.5", item(&id, Some("/Elsewhere")), t(200));
    let next = done.menu_next.unwrap();
    let source = model.menu_source(&next.id).unwrap();
    assert_eq!(source.path, "/Elsewhere");
    // Signals from the old object are not the menu's any more.
    assert_eq!(model.menu_signalled(":1.5", MENU, t(210)), Vec::new());
    model.menu_read_done(&source, layout(&id, vec![entry(1, "Moved")]), t(220));
    assert_eq!(stored(&model, &id)[0].label.text, "Moved");

    // Then it has none.
    model.signalled(":1.5", "/StatusNotifierItem", t(300));
    let done = model.read_done(&id, ":1.5", item(&id, None), t(400));
    assert_eq!(
        done.menu_change,
        Some(Change::MenuFailed(id.clone(), MenuFailure::NoMenu))
    );
    assert_eq!(model.menu(&id), None);
    assert_eq!(model.image_bytes, 0);
}

#[test]
fn signals_of_a_menu_are_paced_and_coalesced() {
    let (mut model, id) = model();
    let source = open(&mut model, &id);
    // During the first read, a storm asks for one more read, no more.
    for ms in 11..60 {
        assert_eq!(model.menu_signalled(":1.5", MENU, t(ms)), Vec::new());
    }
    // Other connections' signals, or another object's, are not this menu's.
    assert_eq!(model.menu_signalled(":1.6", MENU, t(12)), Vec::new());
    let done = model.menu_read_done(&source, layout(&id, vec![entry(1, "Open")]), t(60));
    assert_eq!(done.next.unwrap().at, t(10) + interval());
}

#[test]
fn icons_are_kept_within_the_total_and_released_on_closing() {
    let one = 4 * 4 * 4;
    let (mut model, id) = model_with(Limits {
        max_image_bytes_total: one + 10,
        ..Limits::default()
    });
    let with_icon = |n| {
        let mut e = entry(n, "x");
        e.icon.pixmaps = vec![Arc::new(Image::new(4, 4, vec![0; one]).unwrap())];
        e
    };
    let source = open(&mut model, &id);
    model.menu_read_done(
        &source,
        layout(&id, vec![with_icon(1), with_icon(2)]),
        t(20),
    );
    let entries = stored(&model, &id);
    assert_eq!(entries[0].icon.pixmaps.len(), 1);
    assert_eq!(entries[1].icon.pixmaps.len(), 0);
    assert_eq!(model.image_bytes, one);
    model.close_menu(&id).unwrap();
    assert_eq!(model.image_bytes, 0);
}

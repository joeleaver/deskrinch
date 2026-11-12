//! Menus: opened, read after `AboutToShow`, followed while open, their
//! entries activated only as the caller saw them, and closed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use deskrinch_status_notifier::{
    EntryKind, Handle, Item, Limits, MenuFailure, OperationError, Toggle, ToggleState,
};

use crate::common::{Events, MenuMatches, PrivateBus, STALL, start_with, within};
use crate::flood::{self, Flood, SIGNAL_FLOOD};
use crate::stand_in::{Call, Reply, StandIn, properties, v};
use crate::stand_in_menu::{AboutToShow, Entry, Layout, MenuCall, Prop};

pub(crate) const PATH: &str = "/StatusNotifierItem";
pub(crate) const MENU: &str = "/MenuBar";

/// A host and a stand-in item whose menu at `MENU` answers `layout`.
pub(crate) struct Fixture {
    pub(crate) bus: PrivateBus,
    /// The host's matches for menu signals on the bus.
    pub(crate) matches: MenuMatches,
    pub(crate) handle: Handle,
    pub(crate) events: Events,
    pub(crate) item: StandIn,
    pub(crate) added: Arc<Item>,
    _host: deskrinch_status_notifier::Host,
}

impl Fixture {
    pub(crate) fn start(limits: Limits, layout: Layout) -> Self {
        let bus = PrivateBus::start();
        let matches = MenuMatches::on(&bus);
        let (host, events) = start_with(bus.config(limits));
        let item = StandIn::start(&bus, PATH, Reply::Properties(properties("A")));
        item.set_layout(MENU, layout);
        item.register(&item.unique_name()).unwrap();
        let added = events.added();
        Self {
            bus,
            matches,
            handle: host.handle(),
            events,
            item,
            added,
            _host: host,
        }
    }

    /// Opens the menu and waits for its first reading; the calls that
    /// opening made are taken.
    pub(crate) fn open(&self) -> Arc<deskrinch_status_notifier::Menu> {
        self.handle.open_menu(&self.added.id).unwrap();
        let menu = self.events.menu();
        while self.item.next_menu_call() != MenuCall::GetLayout(MENU.into()) {}
        menu
    }

    /// The next `Event` the menu received, past its reads.
    pub(crate) fn next_event(&self) -> MenuCall {
        loop {
            let call = self.item.next_menu_call();
            if matches!(call, MenuCall::Event(..)) {
                return call;
            }
        }
    }

    /// Points the item's `Menu` at `path`, and says so.
    pub(crate) fn move_menu(&self, path: &str) {
        let mut moved = properties("A");
        moved.insert(
            "Menu".into(),
            v(zbus::wire::ObjectPath::try_from(path).unwrap()),
        );
        self.item.set(Reply::Properties(moved));
        self.item.signal("NewMenu");
    }
}

fn root(entries: Vec<Entry>) -> Layout {
    Layout::Root(Entry::root(entries))
}

#[test]
fn opening_a_menu_says_so_then_reads_it_after_about_to_show() {
    let built = root(vec![
        Entry::labelled(1, "_Fresh")
            .with("toggle-type", Prop::Text("checkmark".into()))
            .with("toggle-state", Prop::Number(1)),
        Entry::labelled(2, "").with("type", Prop::Text("separator".into())),
        Entry::submenu(3, "More", vec![Entry::labelled(4, "Inner")]),
        Entry::labelled(5, "Hidden").with("visible", Prop::Flag(false)),
    ]);
    let f = Fixture::start(Limits::default(), root(vec![Entry::labelled(1, "Stale")]));
    // The application builds its menu when asked to get it ready.
    f.item.menus(|m| {
        m.about_to_show = AboutToShow::Answer {
            needs_update: false,
            after: Duration::ZERO,
            then: Some(built),
        };
    });

    f.handle.open_menu(&f.added.id).unwrap();
    let menu = f.events.menu();
    assert_eq!(
        [
            f.item.next_menu_call(),
            f.item.next_menu_call(),
            f.item.next_menu_call()
        ],
        [
            MenuCall::Event(MENU.into(), 0, "opened".into()),
            MenuCall::AboutToShow(MENU.into(), 0),
            MenuCall::GetLayout(MENU.into()),
        ]
    );
    assert_eq!(menu.item, f.added.id);
    let shown: Vec<_> = menu
        .entries
        .iter()
        .map(|e| (e.kind, e.label.text.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            (EntryKind::Standard, "Fresh"),
            (EntryKind::Separator, ""),
            (EntryKind::Standard, "More"),
        ]
    );
    assert_eq!(menu.entries[0].label.mnemonic, Some(0));
    assert_eq!(menu.entries[0].toggle, Toggle::Checkmark(ToggleState::On));
    let inner = menu.entries[2].submenu.as_ref().unwrap();
    assert_eq!(inner[0].label.text, "Inner");
    assert_eq!(f.handle.menu(&f.added.id), Some(menu));
    // Open already: nothing more.
    f.handle.open_menu(&f.added.id).unwrap();
    f.events.quiet();
    assert_eq!(f.item.menu_calls(), Vec::new());
}

#[test]
fn an_about_to_show_that_never_answers_delays_the_read_by_its_timeout_only() {
    let timeout = Duration::from_millis(300);
    let f = Fixture::start(
        Limits {
            about_to_show_timeout: timeout,
            ..Limits::default()
        },
        root(vec![Entry::labelled(1, "Open")]),
    );
    f.item.menus(|m| m.about_to_show = AboutToShow::Silent);
    let started = Instant::now();
    let menu = f.open();
    assert!(started.elapsed() >= timeout);
    assert_eq!(menu.entries[0].label.text, "Open");
}

#[test]
fn only_an_open_menu_s_signals_are_asked_of_the_bus() {
    let f = Fixture::start(Limits::default(), root(vec![Entry::labelled(1, "Here")]));
    f.item
        .set_layout("/Other", root(vec![Entry::labelled(1, "There")]));
    // Started and an item read, nothing open: the host has asked the bus
    // for no menu's signals.
    assert_eq!(f.matches.live(<[_]>::is_empty), Vec::<String>::new());

    // Asked for when the menu opens, for this menu alone. (The tracker
    // waits for the bus to have it before the first read starts.)
    f.handle.open_menu(&f.added.id).unwrap();
    while f.item.next_menu_call() != MenuCall::AboutToShow(MENU.into(), 0) {}
    let owner = f.item.unique_name();
    let here = |rule: &str, path: &str| {
        rule.contains(&format!("sender='{owner}'")) && rule.contains(&format!("path='{path}'"))
    };
    let live = f.matches.live(|rules| !rules.is_empty());
    assert!(
        matches!(live.as_slice(), [rule] if here(rule, MENU)),
        "{live:?}"
    );
    f.events.menu();

    // The menu moves: the match moves with it.
    f.move_menu("/Other");
    f.events.changed();
    f.events.menu();
    let live = f
        .matches
        .live(|rules| matches!(rules, [rule] if here(rule, "/Other")));
    assert!(
        matches!(live.as_slice(), [rule] if here(rule, "/Other")),
        "{live:?}"
    );

    // Closed: none again.
    f.handle.close_menu(&f.added.id).unwrap();
    assert_eq!(f.matches.live(<[_]>::is_empty), Vec::<String>::new());
}

#[test]
fn a_menu_opened_in_a_flood_of_signals_is_read_and_followed_with_one_match() {
    // Guards: opening a menu awaits no reply (the match for its signals,
    // its read) while the items' stream goes unread, and asks for the
    // match once. zbus stops reading the socket while a stream's queue
    // (64) is full; a flood of item signals running from before the
    // opening until after it fills that queue many times over, so a reply
    // awaited beside the stream unread would not be read: the menu would
    // not come, or the match would be asked for again after a timeout. A
    // stall is turned into a failure by the watchdog. A run in which the
    // host drained the flood before the opening passes without having met
    // the case; one in which it was still flooding, the usual run, shows
    // it.
    within(STALL, || {
        let f = Fixture::start(Limits::default(), root(vec![Entry::labelled(1, "Open")]));
        let flood = Flood::start(&f.bus, SIGNAL_FLOOD, flood::item_signal);
        flood.wait_for(500);
        f.handle.open_menu(&f.added.id).unwrap();
        assert_eq!(f.events.menu().entries[0].label.text, "Open");
        drop(flood);
        let live = f.matches.live(|rules| rules.len() == 1);
        assert_eq!((live.len(), f.matches.added()), (1, 1), "{live:?}");
    });
}

#[test]
fn a_menu_without_about_to_show_is_read_at_once() {
    let timeout = Duration::from_secs(30);
    let f = Fixture::start(
        Limits {
            about_to_show_timeout: timeout,
            ..Limits::default()
        },
        root(vec![Entry::labelled(1, "Open")]),
    );
    f.item.menus(|m| m.about_to_show = AboutToShow::Error);
    let started = Instant::now();
    assert_eq!(f.open().entries[0].label.text, "Open");
    assert!(started.elapsed() < timeout);
}

#[test]
fn an_entry_whose_number_now_names_another_is_refused_not_activated() {
    let f = Fixture::start(
        Limits::default(),
        root(vec![Entry::labelled(1, "Open"), Entry::labelled(2, "Quit")]),
    );
    let menu = f.open();
    let (open, quit) = (menu.entries[0].id, menu.entries[1].id);
    // Numbered by place: "Open" goes, and "Quit" takes its number.
    f.item
        .set_layout(MENU, root(vec![Entry::labelled(1, "Quit")]));
    f.item.layout_updated(MENU);
    let menu = f.events.menu();
    assert_eq!(
        f.handle.activate_entry(&f.added.id, open, None),
        Err(OperationError::EntryUnavailable(open))
    );
    assert_eq!(
        f.handle.activate_entry(&f.added.id, quit, None),
        Err(OperationError::EntryUnavailable(quit))
    );
    let now = menu.entries[0].id;
    f.handle
        .activate_entry(&f.added.id, now, Some("token"))
        .unwrap();
    assert_eq!(f.item.next_call(), Call::Token("token".into()));
    assert_eq!(
        f.next_event(),
        MenuCall::Event(MENU.into(), 1, "clicked".into())
    );
}

#[test]
fn separators_and_disabled_entries_cannot_be_activated() {
    let f = Fixture::start(
        Limits::default(),
        root(vec![
            Entry::labelled(1, "").with("type", Prop::Text("separator".into())),
            Entry::labelled(2, "Off").with("enabled", Prop::Flag(false)),
        ]),
    );
    let menu = f.open();
    for entry in &menu.entries {
        assert_eq!(
            f.handle.activate_entry(&f.added.id, entry.id, None),
            Err(OperationError::EntryUnavailable(entry.id))
        );
    }
}

#[test]
fn closing_says_so_and_stops_following_the_menu() {
    let f = Fixture::start(Limits::default(), root(vec![Entry::labelled(1, "Open")]));
    let menu = f.open();
    f.handle.close_menu(&f.added.id).unwrap();
    assert_eq!(
        f.item.next_menu_call(),
        MenuCall::Event(MENU.into(), 0, "closed".into())
    );
    assert_eq!(f.handle.menu(&f.added.id), None);
    assert_eq!(
        f.handle
            .activate_entry(&f.added.id, menu.entries[0].id, None),
        Err(OperationError::NoMenu(f.added.id.clone()))
    );
    f.item.layout_updated(MENU);
    f.events.quiet();
    assert_eq!(f.item.menu_calls(), Vec::new());
    // Closed already: nothing more.
    f.handle.close_menu(&f.added.id).unwrap();
}

#[test]
fn a_submenu_opened_is_got_ready_and_read_again_when_the_application_asks() {
    let f = Fixture::start(
        Limits::default(),
        root(vec![
            Entry::labelled(1, "Open"),
            Entry::submenu(2, "Recent", Vec::new()),
        ]),
    );
    let menu = f.open();
    let recent = menu.entries[1].id;
    assert_eq!(menu.entries[1].submenu, Some(Vec::new()));
    assert_eq!(
        f.handle.open_submenu(&f.added.id, menu.entries[0].id),
        Err(OperationError::EntryUnavailable(menu.entries[0].id))
    );
    // The application fills the submenu when asked, and says so.
    f.item.menus(|m| {
        m.about_to_show = AboutToShow::Answer {
            needs_update: true,
            after: Duration::ZERO,
            then: Some(root(vec![
                Entry::labelled(1, "Open"),
                Entry::submenu(2, "Recent", vec![Entry::labelled(3, "a.txt")]),
            ])),
        };
    });
    f.handle.open_submenu(&f.added.id, recent).unwrap();
    assert_eq!(
        [
            f.item.next_menu_call(),
            f.item.next_menu_call(),
            f.item.next_menu_call()
        ],
        [
            MenuCall::Event(MENU.into(), 2, "opened".into()),
            MenuCall::AboutToShow(MENU.into(), 2),
            MenuCall::GetLayout(MENU.into()),
        ]
    );
    let menu = f.events.menu();
    assert_eq!(
        menu.entries[1].submenu.as_ref().unwrap()[0].label.text,
        "a.txt"
    );
    // The entry with the submenu is the same entry still.
    assert_eq!(menu.entries[1].id, recent);
    f.handle.close_submenu(&f.added.id, recent).unwrap();
    assert_eq!(
        f.item.next_menu_call(),
        MenuCall::Event(MENU.into(), 2, "closed".into())
    );
}

#[test]
fn a_menu_that_moves_is_read_where_it_is_and_one_that_goes_fails() {
    let f = Fixture::start(Limits::default(), root(vec![Entry::labelled(1, "Here")]));
    f.open();
    f.item
        .set_layout("/Other", root(vec![Entry::labelled(1, "There")]));
    f.move_menu("/Other");
    f.events.changed();
    let menu = f.events.menu();
    assert_eq!(menu.entries[0].label.text, "There");
    // The old object's signals are not the menu's any more.
    f.item.layout_updated(MENU);
    f.events.quiet();

    // `/` is libayatana's way of saying there is none.
    f.move_menu("/");
    assert!(!f.events.changed().has_menu());
    assert_eq!(
        f.events.menu_failed(),
        (f.added.id.clone(), MenuFailure::NoMenu)
    );
    assert_eq!(f.handle.menu(&f.added.id), None);
    assert_eq!(
        f.handle.open_menu(&f.added.id),
        Err(OperationError::NoMenu(f.added.id.clone()))
    );
}

#[test]
fn an_item_that_leaves_takes_its_open_menu_with_it() {
    let Fixture {
        bus: _bus,
        matches,
        handle,
        events,
        item,
        added,
        _host,
    } = Fixture::start(Limits::default(), root(vec![Entry::labelled(1, "Open")]));
    handle.open_menu(&added.id).unwrap();
    let menu = events.menu();
    drop(item);
    assert_eq!(events.removed(), added.id);
    events.quiet();
    assert_eq!(handle.menu(&added.id), None);
    assert_eq!(matches.live(<[_]>::is_empty), Vec::<String>::new());
    assert_eq!(
        handle.open_menu(&added.id),
        Err(OperationError::NotFound(added.id.clone()))
    );
    assert_eq!(
        handle.activate_entry(&added.id, menu.entries[0].id, None),
        Err(OperationError::NotFound(added.id.clone()))
    );
}

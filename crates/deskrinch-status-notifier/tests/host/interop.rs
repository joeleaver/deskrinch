//! A real item, made with ksni, read and activated.
//!
//! ksni connects only to the session bus, so the item runs in a child
//! process: this test binary's ignored `interop::ksni_item`, started in the
//! private bus's isolated session, where the session bus is the test's. It
//! reports the calls it receives on its standard output.

use std::io::{BufRead as _, BufReader, Read as _};
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use deskrinch_status_notifier::{
    Activation, Category, EntryKind, OperationError, Orientation, Status, Toggle, ToggleState,
};

use crate::common::{PATIENCE, PrivateBus, start};

/// Set in the child's environment, so that the ignored test runs only
/// there.
const CHILD: &str = "DESKRINCH_KSNI_ITEM";

/// What the child prints before each call it reports.
const MARK: &str = "ksni-item:";

/// The ksni item; not a test of its own.
#[test]
#[ignore = "run by interop::a_ksni_item_is_read_and_activated in a child process"]
fn ksni_item() {
    use ksni::blocking::TrayMethods as _;

    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let handle = Tray { muted: true }.spawn().unwrap();
    // Until the test closes standard input.
    let _read = std::io::stdin().read_to_end(&mut Vec::new());
    handle.shutdown().wait();
}

struct Tray {
    muted: bool,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "deskrinch-test".into()
    }

    fn title(&self) -> String {
        "Test tray".into()
    }

    fn status(&self) -> ksni::Status {
        ksni::Status::NeedsAttention
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Communications
    }

    fn icon_name(&self) -> String {
        "mail-unread".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![ksni::Icon {
            width: 1,
            height: 1,
            data: vec![0xff, 0x10, 0x20, 0x30],
        }]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
            title: "Tip".into(),
            description: "<b>Two</b> new".into(),
        }
    }

    fn activate(&mut self, x: i32, y: i32) {
        let token = ksni::current_xdg_activation_token().unwrap_or_default();
        println!("{MARK} activate {x} {y} {token}");
    }

    fn secondary_activate(&mut self, x: i32, y: i32) {
        println!("{MARK} secondary {x} {y}");
    }

    fn scroll(&mut self, delta: i32, orientation: ksni::Orientation) {
        println!("{MARK} scroll {delta} {orientation:?}");
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{CheckmarkItem, RadioGroup, RadioItem, StandardItem, SubMenu};
        let radio = |label: &str| RadioItem {
            label: label.into(),
            ..RadioItem::default()
        };
        vec![
            StandardItem {
                label: "_Open".into(),
                icon_name: "document-open".into(),
                shortcut: vec![vec!["Control".into(), "o".into()]],
                activate: Box::new(|_| {
                    let token = ksni::current_xdg_activation_token().unwrap_or_default();
                    println!("{MARK} menu open {token}");
                }),
                ..StandardItem::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            CheckmarkItem {
                label: "Mute".into(),
                checked: self.muted,
                activate: Box::new(|tray: &mut Self| {
                    tray.muted = !tray.muted;
                    println!("{MARK} menu mute {}", tray.muted);
                }),
                ..CheckmarkItem::default()
            }
            .into(),
            SubMenu {
                label: "Mode".into(),
                submenu: vec![
                    RadioGroup {
                        selected: 1,
                        select: Box::new(|_, i| println!("{MARK} menu mode {i}")),
                        options: vec![radio("A"), radio("B")],
                    }
                    .into(),
                ],
                ..SubMenu::default()
            }
            .into(),
            StandardItem {
                label: "Off".into(),
                enabled: false,
                ..StandardItem::default()
            }
            .into(),
            StandardItem {
                label: "Hidden".into(),
                visible: false,
                ..StandardItem::default()
            }
            .into(),
        ]
    }
}

/// The child process, ended when dropped.
struct Item {
    child: Child,
    lines: Receiver<String>,
}

impl Item {
    fn start(bus: &PrivateBus) -> Self {
        let mut command = bus.command(std::env::current_exe().unwrap());
        command
            .args(["--exact", "interop::ksni_item", "--ignored", "--nocapture"])
            .env(CHILD, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = bus.spawn(command).unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(call) = line.strip_prefix(MARK)
                    && tx.send(call.trim().to_owned()).is_err()
                {
                    return;
                }
            }
        });
        Self { child, lines }
    }

    fn next_call(&self) -> String {
        self.lines
            .recv_timeout(PATIENCE)
            .expect("the ksni item reported no call in time")
    }
}

impl Drop for Item {
    fn drop(&mut self) {
        // Closing its standard input ends it; if it is stuck, it is killed.
        drop(self.child.stdin.take());
        let _killed = self.child.kill();
        let _waited = self.child.wait();
    }
}

#[test]
fn a_ksni_item_is_read_and_activated() {
    let bus = PrivateBus::start();
    let (host, events) = start(&bus);
    let item = Item::start(&bus);

    let added = events.added();
    assert!(
        added.id.as_str().starts_with("org.kde.StatusNotifierItem-"),
        "{}",
        added.id
    );
    assert!(added.id.as_str().ends_with("/StatusNotifierItem"));
    assert_eq!(added.app_id, "deskrinch-test");
    assert_eq!(added.title, "Test tray");
    assert_eq!(added.status, Status::NeedsAttention);
    assert_eq!(added.category, Category::Communications);
    assert_eq!(added.icon.name.as_ref().unwrap().as_str(), "mail-unread");
    assert_eq!(added.icon.pixmaps[0].rgba(), &[0x10, 0x20, 0x30, 0xff]);
    let tooltip = added.tooltip.as_ref().unwrap();
    assert_eq!(tooltip.title, "Tip");
    assert_eq!(tooltip.description, "Two new");
    assert!(added.has_menu());
    assert!(!added.item_is_menu);

    let handle = host.handle();
    handle
        .activate(&added.id, Activation::Primary { x: 4, y: 5 }, Some("tok"))
        .unwrap();
    assert_eq!(item.next_call(), "activate 4 5 tok");
    handle
        .activate(&added.id, Activation::Secondary { x: 6, y: 7 }, None)
        .unwrap();
    assert_eq!(item.next_call(), "secondary 6 7");
    let scroll = Activation::Scroll {
        delta: 2,
        orientation: Orientation::Vertical,
    };
    handle.activate(&added.id, scroll, None).unwrap();
    assert_eq!(item.next_call(), "scroll 2 Vertical");
}

#[test]
fn a_ksni_menu_is_read_activated_and_followed() {
    let bus = PrivateBus::start();
    let (host, events) = start(&bus);
    let item = Item::start(&bus);
    let added = events.added();
    let handle = host.handle();

    handle.open_menu(&added.id).unwrap();
    let menu = events.menu();
    let shown: Vec<_> = menu
        .entries
        .iter()
        .map(|e| (e.kind, e.label.text.as_str(), e.enabled, e.toggle))
        .collect();
    assert_eq!(
        shown,
        [
            (EntryKind::Standard, "Open", true, Toggle::None),
            (EntryKind::Separator, "", true, Toggle::None),
            (
                EntryKind::Standard,
                "Mute",
                true,
                Toggle::Checkmark(ToggleState::On)
            ),
            (EntryKind::Standard, "Mode", true, Toggle::None),
            (EntryKind::Standard, "Off", false, Toggle::None),
        ]
    );
    let open = &menu.entries[0];
    assert_eq!(open.label.mnemonic, Some(0));
    assert_eq!(open.icon.name.as_ref().unwrap().as_str(), "document-open");
    assert_eq!(open.shortcuts[0].keys, ["Control", "o"]);
    let modes: Vec<_> = menu.entries[3]
        .submenu
        .as_ref()
        .unwrap()
        .iter()
        .map(|e| (e.label.text.as_str(), e.toggle))
        .collect();
    assert_eq!(
        modes,
        [
            ("A", Toggle::Radio(ToggleState::Off)),
            ("B", Toggle::Radio(ToggleState::On)),
        ]
    );

    handle
        .activate_entry(&added.id, open.id, Some("tok"))
        .unwrap();
    assert_eq!(item.next_call(), "menu open tok");
    let a = menu.entries[3].submenu.as_ref().unwrap()[0].id;
    handle.open_submenu(&added.id, menu.entries[3].id).unwrap();
    handle.activate_entry(&added.id, a, None).unwrap();
    assert_eq!(item.next_call(), "menu mode 0");
    assert_eq!(
        handle.activate_entry(&added.id, menu.entries[4].id, None),
        Err(OperationError::EntryUnavailable(menu.entries[4].id))
    );

    // Toggling changes the menu, which ksni announces: it is read again,
    // and the entry that changed only its state is the same entry.
    handle
        .activate_entry(&added.id, menu.entries[2].id, None)
        .unwrap();
    assert_eq!(item.next_call(), "menu mute false");
    let changed = loop {
        let menu = events.menu();
        if menu.entries[2].toggle == Toggle::Checkmark(ToggleState::Off) {
            break menu;
        }
    };
    assert_eq!(changed.entries[2].id, menu.entries[2].id);
    handle
        .activate_entry(&added.id, changed.entries[2].id, None)
        .unwrap();
    assert_eq!(item.next_call(), "menu mute true");
    handle.close_menu(&added.id).unwrap();
}

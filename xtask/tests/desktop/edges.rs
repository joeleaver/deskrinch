//! KWin's screen edges and its Overview, in the desktop's KWin: the
//! Overview opens by its shortcut, shows the windows, and closes again;
//! then pushing the pointer into every corner and along every edge finds
//! the Overview's hot corner (top-left) and nothing else, opens the
//! Overview there, and leaves KWin and the shell running.
//!
//! Both wait until KWin has loaded the Overview, which it does after it has
//! started: before then its shortcut does nothing, and a push into the
//! corner only lights it (KWin's `screenedge` effect).
//!
//! The pointer has to be the nested KWin's own, so the checks run inside
//! the desktop: the outer test starts it on a profile of its own that
//! grants this binary KWin's fake input and screenshots (a desktop file in
//! the profile's applications), with this binary, filtered to
//! [`inside_the_desktop_edges_and_the_overview`], as the command after
//! `--`. That one runs only there ([`INSIDE`]); anywhere else it has
//! nothing to check and says so.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use deskrinch_kwin_access::DesktopEntry;
use deskrinch_test_client::{Compositor, Paint, TestClient};
use deskrinch_test_input::{Input, KWIN_FAKE_INPUT_INTERFACE, Key};
use deskrinch_test_screen::{KWIN_SCREENSHOT_INTERFACE, Rgba, Screen};

use crate::support::{TestProfile, start};

/// Set to the file the inside test writes its findings to: it runs only
/// where this is set.
const INSIDE: &str = "DESKRINCH_DESKTOP_EDGES_RECORD";
/// The inside test, by its libtest name.
const INSIDE_TEST: &str = "edges::inside_the_desktop_edges_and_the_overview";
/// A colour for the window the Overview shows.
const SEEN: Rgba = Rgba::opaque(0xe0, 0x7a, 0x1f);
/// How long anything inside is waited for.
const PATIENCE: Duration = Duration::from_secs(30);
/// How long the pointer keeps pushing against an edge: well past the time
/// KWin waits there before it acts (`ElectricBorderDelay`, 75 ms by
/// default).
const PUSHING: Duration = Duration::from_millis(1500);
/// How often a push repeats while it lasts, as a mouse's motion does.
const PUSH_EVERY: Duration = Duration::from_millis(20);

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn kwin_s_edges_and_overview_leave_it_and_the_shell_running() {
    if Compositor::current() != Compositor::Kwin {
        // KWin's own edges and effects: nothing of the kind elsewhere.
        return;
    }
    let profile = TestProfile::new();
    let this = std::env::current_exe().unwrap();
    grant(&profile.dir, &this);
    let record = profile.dir.with_extension("record");
    let script = format!(
        "{INSIDE}='{}' exec '{}' --ignored --exact {INSIDE_TEST}",
        record.display(),
        this.display()
    );
    let mut desktop = start(
        &["--profile", profile.name.as_str()],
        &["sh", "-c", &script],
    );
    let status = desktop.wait_within(Duration::from_secs(300));
    let found = fs::read_to_string(&record).unwrap_or_default();
    let _removed = fs::remove_file(&record);
    eprintln!("found inside:\n{found}");
    assert!(
        status.success(),
        "the checks inside failed ({status}); the desktop said:\n{}",
        desktop.said()
    );
    assert!(
        !desktop.said().contains(" exited ("),
        "a component ended; the desktop said:\n{}",
        desktop.said()
    );
    desktop.wait_for_root_gone();
}

/// Grants `binary` KWin's fake input and screenshots, in the profile at
/// `profile`: an application of its applications, which KWin reads.
fn grant(profile: &Path, binary: &Path) {
    let entry = DesktopEntry::new(
        "org.deskrinch.DesktopEdgesTests",
        "deskrinch desktop edge tests",
        binary,
        &[KWIN_FAKE_INPUT_INTERFACE],
        &[KWIN_SCREENSHOT_INTERFACE],
    )
    .unwrap();
    entry.install(&profile.join("data")).unwrap();
}

/// What the inside test found, written as it goes, so that a failure
/// still shows what came before it.
struct Record(PathBuf);

impl Record {
    fn note(&self, line: &str) {
        let mut text = fs::read_to_string(&self.0).unwrap_or_default();
        text.push_str(line);
        text.push('\n');
        fs::write(&self.0, text).unwrap();
    }
}

/// The effects KWin has active, on the session's bus.
fn active_effects() -> Vec<String> {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    zbus::block_on(async {
        let connection = zbus::connection::Builder::address(address.as_str())
            .build()
            .await
            .unwrap();
        let reply = connection
            .call_method(
                Some("org.kde.KWin"),
                "/Effects",
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.kde.kwin.Effects", "activeEffects"),
            )
            .await
            .unwrap();
        let value: zbus::OwnedVariant = reply.body().deserialize().unwrap();
        <Vec<String>>::try_from(value).unwrap()
    })
}

/// Calls `method` with no arguments on `path`'s `interface` of `name`, on
/// the session's bus; its reply's body, or why there is none.
fn call(name: &str, path: &str, interface: &str, method: &str, args: &str) -> Result<(), String> {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    zbus::block_on(async {
        let connection = zbus::connection::Builder::address(address.as_str())
            .build()
            .await
            .map_err(|error| error.to_string())?;
        let reply = if args.is_empty() {
            connection
                .call_method(Some(name), path, Some(interface), method, &())
                .await
        } else {
            connection
                .call_method(Some(name), path, Some(interface), method, &(args,))
                .await
        };
        reply.map(drop).map_err(|error| error.to_string())
    })
}

/// Whether KWin has loaded the effect `name`: it loads its effects after it
/// has started, and an effect not loaded yet neither takes its shortcut
/// nor opens from its edge.
fn effect_loaded(name: &str) -> bool {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    zbus::block_on(async {
        let connection = zbus::connection::Builder::address(address.as_str())
            .build()
            .await
            .unwrap();
        let reply = connection
            .call_method(
                Some("org.kde.KWin"),
                "/Effects",
                Some("org.kde.kwin.Effects"),
                "isEffectLoaded",
                &(name,),
            )
            .await
            .unwrap();
        reply.body().deserialize::<bool>().unwrap()
    })
}

/// Whether KWin's global shortcut `name` is registered with its
/// shortcut daemon.
fn shortcut_registered(name: &str) -> bool {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    zbus::block_on(async {
        let connection = zbus::connection::Builder::address(address.as_str())
            .build()
            .await
            .unwrap();
        let reply = connection
            .call_method(
                Some("org.kde.kglobalaccel"),
                "/component/kwin",
                Some("org.kde.kglobalaccel.Component"),
                "shortcutNames",
                &(),
            )
            .await;
        reply
            .ok()
            .and_then(|reply| reply.body().deserialize::<Vec<String>>().ok())
            .is_some_and(|names| names.iter().any(|known| known == name))
    })
}

/// Whether KWin still answers on the session's bus.
fn kwin_running() -> bool {
    call(
        "org.kde.KWin",
        "/KWin",
        "org.kde.KWin",
        "currentDesktop",
        "",
    )
    .is_ok()
}

/// Invokes KWin's global shortcut `name`, as its keys would.
fn invoke(name: &str) {
    call(
        "org.kde.kglobalaccel",
        "/component/kwin",
        "org.kde.kglobalaccel.Component",
        "invokeShortcut",
        name,
    )
    .unwrap();
}

/// Opens the Overview by its shortcut. For a few seconds after KWin has
/// loaded it (its QML is still being made, which nothing on the bus
/// tells), the shortcut does nothing: measured, the first two of six
/// invocations two seconds apart from the moment it was loaded opened
/// nothing, and every one after toggled it. So it is invoked until it
/// opens, each time only once the last invocation has had two seconds to
/// show it: never while it is open, which would close it.
fn open_overview_by_its_shortcut() {
    let deadline = Instant::now() + PATIENCE;
    loop {
        invoke("Overview");
        let shown_by = Instant::now() + Duration::from_secs(2);
        while Instant::now() < shown_by {
            if active_effects().iter().any(|e| e == "overview") {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            Instant::now() < deadline,
            "the Overview's shortcut never opened it"
        );
    }
}

/// Waits until `effects` holds for the active effects; the last seen.
fn wait_effects(what: &str, effects: impl Fn(&[String]) -> bool) -> Vec<String> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let active = active_effects();
        if effects(&active) {
            return active;
        }
        assert!(
            Instant::now() < deadline,
            "waited in vain for {what}: {active:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Pushes the pointer into `(x, y)` on `output`, at an edge or corner, as
/// a hand on a mouse does: there, then on outwards in small motions for
/// [`PUSHING`], which the compositor stops at the edge.
fn push(input: &mut Input, output: &str, (x, y): (f64, f64), (width, height): (f64, f64)) {
    // Outwards on each axis the point is at an edge of.
    let outwards = |at: f64, end: f64| {
        if at <= 0.0 {
            -4.0
        } else if at >= end - 1.0 {
            4.0
        } else {
            0.0
        }
    };
    let (dx, dy) = (outwards(x, width), outwards(y, height));
    input.move_pointer(output, x, y).unwrap();
    let until = Instant::now() + PUSHING;
    while Instant::now() < until {
        input.nudge_pointer(dx, dy).unwrap();
        std::thread::sleep(PUSH_EVERY);
    }
    input
        .move_pointer(output, width / 2.0, height / 2.0)
        .unwrap();
}

/// Whether some pixel of `output`'s picture is [`SEEN`]: the window's
/// thumbnail is drawn.
fn shows_the_window(screen: &mut Screen, output: &str) -> bool {
    let image = screen.capture(output).unwrap();
    image.pixels().iter().any(|&pixel| {
        pixel.r.abs_diff(SEEN.r) <= 2
            && pixel.g.abs_diff(SEEN.g) <= 2
            && pixel.b.abs_diff(SEEN.b) <= 2
    })
}

#[test]
#[ignore = "runs inside the desktop that kwin_s_edges_and_overview_leave_it_and_the_shell_running starts"]
fn inside_the_desktop_edges_and_the_overview() {
    let Some(record) = std::env::var_os(INSIDE) else {
        eprintln!("not inside the desktop the edge test starts: nothing to check here");
        return;
    };
    let record = Record(PathBuf::from(record));
    // A failure here is said in the record too: this test's own output
    // goes where the desktop's does, which the outer test does not show.
    let failures = record.0.clone();
    std::panic::set_hook(Box::new(move |info| {
        Record(failures.clone()).note(&format!("failed: {info}"));
    }));
    let mut input = Input::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let output = client.output_names().remove(0);
    let (width, height) = client.output_info(&output).unwrap().logical_size.unwrap();
    let size = (f64::from(width), f64::from(height));
    let _window = client
        .open_painted(
            "Seen in the overview",
            "org.deskrinch.test.Overview",
            Paint::Solid(SEEN),
            None,
        )
        .unwrap();
    // The Overview by its shortcut, showing the window, and closed again,
    // once KWin has loaded it.
    let deadline = Instant::now() + PATIENCE;
    while !effect_loaded("overview") || !shortcut_registered("Overview") {
        assert!(
            Instant::now() < deadline,
            "KWin never loaded the Overview, or never registered its shortcut"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    // And once the window's opening has played out.
    let before = wait_effects("the window's opening to end", |now| {
        now.iter().all(|e| e != "scale")
    });
    record.note(&format!("before: {before:?}"));
    open_overview_by_its_shortcut();
    std::thread::sleep(Duration::from_secs(1));
    let mut screen = Screen::connect().unwrap();
    let shown = shows_the_window(&mut screen, &output);
    record.note(&format!("the Overview shows the window: {shown}"));
    invoke("Overview");
    wait_effects("the Overview to close", |now| {
        now.iter().all(|e| e != "overview")
    });
    assert!(kwin_running(), "KWin is gone after the Overview");
    assert!(shown, "the Overview did not show the window");
    let baseline = active_effects();
    let (right, bottom) = (size.0 - 1.0, size.1 - 1.0);
    let places = [
        ("top-left corner", (0.0, 0.0)),
        ("top edge", (size.0 / 2.0, 0.0)),
        ("top-right corner", (right, 0.0)),
        ("right edge", (right, size.1 / 2.0)),
        ("bottom-right corner", (right, bottom)),
        ("bottom edge", (size.0 / 2.0, bottom)),
        ("bottom-left corner", (0.0, bottom)),
        ("left edge", (0.0, size.1 / 2.0)),
    ];
    // Each place pushed, and what started: KWin's `screenedge` effect lights
    // an edge reserved for an action as the pointer comes to it.
    let (mut lit, mut opened) = (Vec::new(), Vec::new());
    for (place, point) in places {
        push(&mut input, &output, point, size);
        std::thread::sleep(Duration::from_secs(1));
        assert!(kwin_running(), "KWin is gone after the {place}");
        let active = active_effects();
        let started: Vec<_> = active.iter().filter(|e| !baseline.contains(e)).collect();
        record.note(&format!("{place}: {started:?}"));
        if started.iter().any(|e| *e == "screenedge") {
            lit.push(place);
        }
        if started.iter().any(|e| *e == "overview") {
            opened.push(place);
        }
        if started.iter().any(|e| *e != "screenedge") {
            input.press(Key::ESCAPE).unwrap();
        }
        wait_effects("what started to end", |now| {
            now.iter().all(|e| baseline.contains(e))
        });
    }
    // The Overview's hot corner is there and opens it (closed by Escape),
    // and no other edge does anything.
    assert_eq!(lit, ["top-left corner"]);
    assert_eq!(opened, ["top-left corner"]);
    // The shell still answers: the launcher toggles, both ways.
    for _ in 0..2 {
        call(
            "org.deskrinch.Launcher",
            "/org/deskrinch/Launcher",
            "org.deskrinch.Launcher1",
            "Toggle",
            "",
        )
        .unwrap();
    }
    record.note("the launcher still answers");
}

//! The window type KWin gives a toplevel, and a layer surface by its
//! namespace alone, with the layer it stacks it in and what leaves it out.
//!
//! The table is what KWin 6.6.7 gave in `cargo xtask session kwin`
//! (2026-11-23), read through this crate with the test client's surfaces,
//! each namespace on the top and the overlay layer. Two findings it holds:
//! KWin's key for a critical notification is spelt `crititical-notification`
//! (the correct spelling gives an ordinary window), and a layer surface's
//! layer follows its layer-shell layer (top 3, overlay 9), not its type.

use std::collections::BTreeSet;

use deskrinch_test_client::{Compositor, ShellLayer, TestClient};
use deskrinch_test_kwin::{Keep, Skip, Window, WindowType, version, windows};

/// The process's own windows of `size`, among every window KWin has.
fn of_size(windows: &[Window], pid: u32, size: (u32, u32)) -> Vec<&Window> {
    windows
        .iter()
        .filter(|window| {
            window.pid == pid && window.size() == (f64::from(size.0), f64::from(size.1))
        })
        .collect()
}

/// Each namespace and the type KWin gives a layer surface of it.
const NAMESPACES: [(&str, WindowType); 13] = [
    ("desktop", WindowType::Desktop),
    ("dock", WindowType::Dock),
    ("notification", WindowType::Notification),
    ("crititical-notification", WindowType::CriticalNotification),
    ("critical-notification", WindowType::Normal),
    ("on-screen-display", WindowType::OnScreenDisplay),
    ("tooltip", WindowType::Tooltip),
    ("dialog", WindowType::Dialog),
    ("splash", WindowType::Splash),
    ("utility", WindowType::Utility),
    ("appletpopup", WindowType::Normal),
    ("deskrinch-unknown", WindowType::Normal),
    ("", WindowType::Normal),
];

/// The layer KWin stacks a layer surface on each layer-shell layer in.
const LAYERS: [(ShellLayer, i64); 2] = [(ShellLayer::Top, 3), (ShellLayer::Overlay, 9)];

/// What leaves a layer surface of `kind` out: everything, but a switcher
/// still offers the desktop.
fn skips_of(kind: WindowType) -> BTreeSet<Skip> {
    if kind == WindowType::Desktop {
        BTreeSet::from([Skip::Taskbar, Skip::Pager])
    } else {
        BTreeSet::from([Skip::Taskbar, Skip::Pager, Skip::Switcher])
    }
}

/// An ordinary toplevel, opened after a layer surface, is a normal window
/// on the ordinary layer, and nothing leaves it out.
#[test]
#[ignore = "needs a nested KWin: cargo xtask compositor-tests kwin"]
fn an_ordinary_toplevel_is_a_normal_window() {
    assert_eq!(Compositor::current(), Compositor::Kwin);
    let kwin = version().unwrap();
    let client = TestClient::connect().unwrap();
    let _before = client
        .open_layer("dock", ShellLayer::Top, (33, 17))
        .unwrap();
    let _toplevel = client
        .open(
            "deskrinch-test-kwin toplevel",
            "org.deskrinch.TestKwin.Toplevel",
        )
        .unwrap();
    let all = windows().unwrap();
    let mine: Vec<&Window> = all
        .iter()
        .filter(|window| window.resource_class == "org.deskrinch.TestKwin.Toplevel")
        .collect();
    let [toplevel] = mine[..] else {
        panic!("KWin {kwin}: one toplevel of ours expected among {all:#?}");
    };
    assert_eq!(
        toplevel.caption, "deskrinch-test-kwin toplevel",
        "KWin {kwin}"
    );
    assert_eq!(toplevel.window_type, WindowType::Normal, "KWin {kwin}");
    assert_eq!(
        toplevel.flags,
        BTreeSet::from([WindowType::Normal]),
        "KWin {kwin}"
    );
    assert_eq!(toplevel.layer, 2, "KWin {kwin}");
    assert_eq!(toplevel.keep, Keep::Neither, "KWin {kwin}");
    assert!(
        toplevel.skips.is_empty(),
        "KWin {kwin}: {:?}",
        toplevel.skips
    );
}

/// Every namespace of the table, on the top and the overlay layer, after a
/// toplevel: each surface has a size of its own, by which it is found.
/// Every row that differs is named, with KWin's version.
#[test]
#[ignore = "needs a nested KWin: cargo xtask compositor-tests kwin"]
fn a_layer_surface_is_typed_by_its_namespace() {
    assert_eq!(Compositor::current(), Compositor::Kwin);
    let kwin = version().unwrap();
    let client = TestClient::connect().unwrap();
    let _first = client
        .open("deskrinch-test-kwin first", "org.deskrinch.TestKwin.First")
        .unwrap();
    let mut held = Vec::new();
    let mut rows = Vec::new();
    for (l, (layer, kwin_layer)) in (0u32..).zip(LAYERS) {
        for (n, (namespace, kind)) in (0u32..).zip(NAMESPACES) {
            let size = (40 + n, 20 + l);
            held.push(client.open_layer(namespace, layer, size).unwrap());
            rows.push((namespace, layer, size, kind, kwin_layer));
        }
    }
    let all = windows().unwrap();
    let pid = std::process::id();
    let differing: Vec<String> = rows
        .into_iter()
        .filter_map(|(namespace, layer, size, kind, kwin_layer)| {
            let found = of_size(&all, pid, size);
            let [window] = found[..] else {
                return Some(format!(
                    "{namespace:?} on {layer:?}: {} windows",
                    found.len()
                ));
            };
            let expected = (
                kind,
                BTreeSet::from([kind]),
                kwin_layer,
                Keep::Neither,
                skips_of(kind),
            );
            let got = (
                window.window_type,
                window.flags.clone(),
                window.layer,
                window.keep,
                window.skips.clone(),
            );
            (got != expected).then(|| {
                format!("{namespace:?} on {layer:?}: expected {expected:?}, KWin gave {got:?}")
            })
        })
        .collect();
    assert!(
        differing.is_empty(),
        "KWin {kwin} differs from the table read on KWin 6.6.7 (type, flags, layer, keep, skips):\n{}",
        differing.join("\n")
    );
}

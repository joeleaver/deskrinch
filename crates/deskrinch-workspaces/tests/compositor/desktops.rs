//! KWin's desktops changed by KWin's own interface: created, removed, and
//! laid out in rows, as the crate reports them. (labwc's workspaces are
//! fixed by its configuration: the suites table runs these under KWin
//! only.)

use deskrinch_workspaces::WorkspaceState;

use crate::support::{COUNT, Compositor, Watcher, compositor, kwin, set_up};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_desktop_created_and_removed_by_kwin_comes_and_goes() {
    let compositor = compositor();
    assert_eq!(
        compositor,
        Compositor::Kwin,
        "the suites table runs these under KWin"
    );
    let watcher = Watcher::connect();
    set_up(compositor, &watcher);
    let name = format!("Extra {}", std::process::id());
    kwin::create(&name);
    let added = watcher.wait_until("the new desktop", |workspaces| {
        workspaces
            .iter()
            .find(|workspace| workspace.name == name)
            .cloned()
    });
    assert!(!added.state.contains(WorkspaceState::ACTIVE));
    assert_eq!(watcher.listed().last().map(|last| &last.name), Some(&name));
    kwin::remove(&name);
    watcher.wait_until("the desktop gone", |workspaces| {
        workspaces
            .iter()
            .all(|workspace| workspace.name != name)
            .then_some(())
    });
    assert!(
        watcher
            .listed()
            .iter()
            .all(|workspace| workspace.name != name)
    );
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn kwins_rows_lay_the_desktops_out() {
    let compositor = compositor();
    assert_eq!(
        compositor,
        Compositor::Kwin,
        "the suites table runs these under KWin"
    );
    let watcher = Watcher::connect();
    set_up(compositor, &watcher);
    kwin::set_rows(2);
    watcher.wait_until("two rows", |workspaces| {
        workspaces
            .iter()
            .any(|workspace| workspace.coordinates.first() == Some(&1))
            .then_some(())
    });
    let listed = watcher.listed();
    assert_eq!(listed.len(), COUNT, "{listed:#?}");
    let cells: Vec<_> = listed
        .iter()
        .map(|workspace| workspace.coordinates.clone())
        .collect();
    assert_eq!(cells, [[0, 0], [0, 1], [1, 0], [1, 1]]);
    kwin::set_rows(1);
}

//! The workspaces listed: in order, named, exactly one active, in a group on
//! the session's outputs, with what the protocol reports.

use deskrinch_test_client::TestClient;
use deskrinch_workspaces::{Action, Actions, Protocol, WorkspaceState, Workspaces};

use crate::support::{COUNT, Compositor, Watcher, compositor, set_up};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn lists_the_workspaces_in_order_with_the_active_one() {
    let compositor = compositor();
    let watcher = Watcher::connect();
    set_up(compositor, &watcher);
    let first = watcher.listed()[0].name.clone();
    watcher.wait_active(&first);

    let listed = watcher.listed();
    assert!(listed.len() >= COUNT, "{listed:#?}");
    assert!(listed.iter().all(|workspace| !workspace.name.is_empty()));
    let active: Vec<_> = listed
        .iter()
        .filter(|workspace| workspace.state.contains(WorkspaceState::ACTIVE))
        .collect();
    assert_eq!(active.len(), 1, "{listed:#?}");
    assert_eq!(active[0].name, first);
    assert!(
        listed
            .iter()
            .all(|workspace| workspace.actions.contains(Actions::ACTIVATE)),
        "every workspace may be activated: {listed:#?}"
    );

    // One group, on every output of the session, which every workspace is in.
    let groups = watcher.manager.groups();
    assert_eq!(groups.len(), 1, "{groups:#?}");
    let outputs: std::collections::BTreeSet<String> = TestClient::connect()
        .unwrap()
        .output_names()
        .into_iter()
        .collect();
    assert_eq!(groups[0].outputs, outputs);
    assert!(
        listed
            .iter()
            .all(|workspace| workspace.group == Some(groups[0].id))
    );

    let capabilities = watcher.manager.capabilities();
    assert!(capabilities.supports(Action::Activate));
    match compositor {
        Compositor::Kwin => {
            assert_eq!(capabilities.protocol, Protocol::PlasmaVirtualDesktop);
            assert_eq!(capabilities.states, WorkspaceState::ACTIVE);
            // One row: the position is the column.
            for (column, workspace) in listed.iter().enumerate() {
                assert_eq!(
                    workspace.coordinates,
                    [0, u32::try_from(column).unwrap()],
                    "{listed:#?}"
                );
            }
        }
        Compositor::Labwc => {
            assert_eq!(capabilities.protocol, Protocol::ExtWorkspace);
            assert_eq!(listed.len(), COUNT, "the sessions' labwc has four");
            // labwc sends no coordinates.
            assert!(
                listed
                    .iter()
                    .all(|workspace| workspace.coordinates.is_empty())
            );
        }
        Compositor::Sway => panic!("sway runs only the unsupported tests"),
    }
}

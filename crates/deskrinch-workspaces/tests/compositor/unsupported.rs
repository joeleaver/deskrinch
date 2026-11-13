//! sway (1.11) offers no workspace protocol: its workspaces are reachable
//! only through its IPC. The suites table runs this under sway only.

use deskrinch_workspaces::{ConnectError, connect};

use crate::support::{Compositor, compositor};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn sway_offers_no_workspace_protocol() {
    assert_eq!(
        compositor(),
        Compositor::Sway,
        "the suites table runs this under sway"
    );
    let error = connect(|_| {}).unwrap_err();
    assert!(matches!(error, ConnectError::Unsupported), "{error:?}");
}

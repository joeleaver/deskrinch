//! KWin withholds its window-management protocol from a client that no
//! installed desktop file grants it to. `cargo xtask compositor-tests` runs
//! this binary in the nested KWin without installing one for it.

#[path = "support/compositor.rs"]
mod compositor;

use compositor::Compositor;
use deskrinch_window_management::{ConnectError, connect};

#[test]
#[ignore = "needs a nested KWin: cargo xtask compositor-tests"]
fn kwin_refuses_a_client_without_a_desktop_file() {
    assert_eq!(Compositor::current(), Compositor::Kwin);
    let error = connect(|_| {}).expect_err("KWin granted an ungranted client");
    assert!(matches!(error, ConnectError::NotGranted), "{error:?}");
}

//! KWin answers its screenshot interface only for an executable that an
//! installed desktop file grants it to. `cargo xtask compositor-tests` runs
//! this binary in the nested KWin without installing one for it.

use deskrinch_test_client::Compositor;
use deskrinch_test_screen::{CaptureError, Method, Screen};

#[test]
#[ignore = "needs a nested KWin: cargo xtask compositor-tests"]
fn kwin_refuses_a_capture_without_a_desktop_file() {
    assert_eq!(Compositor::current(), Compositor::Kwin);
    let mut screen = Screen::connect().unwrap();
    assert_eq!(screen.method(), Method::KwinScreenShot2);
    let output = screen.outputs()[0].clone();
    let error = screen
        .capture(&output)
        .expect_err("KWin answered an ungranted caller");
    assert!(matches!(error, CaptureError::NotAuthorized(_)), "{error:?}");
}

//! A test window opened on a chosen output: the pointer moved onto that
//! output first, where each compositor opens a new window (sway's focus
//! follows the pointer, KWin's active screen follows the mouse, labwc
//! places a window at the cursor's output), and the window asked where it
//! went (`TestWindow::wait_output`), never assumed.

use deskrinch_test_client::{TestClient, TestWindow};
use deskrinch_test_input::Input;

/// A window titled `title`, of `app_id`, opened on `output`, once it says
/// it is there.
///
/// # Panics
///
/// When it opens on another output, naming it.
pub(crate) fn open_on(
    input: &mut Input,
    client: &TestClient,
    output: &str,
    title: &str,
    app_id: &str,
) -> TestWindow {
    let (width, height) = client
        .output_info(output)
        .and_then(|info| info.logical_size)
        .unwrap_or_else(|| panic!("{output}'s logical size"));
    input
        .move_pointer(output, f64::from(width) / 2.0, f64::from(height) / 2.0)
        .unwrap();
    let window = client.open(title, app_id).unwrap();
    let on = window.wait_output().unwrap();
    assert_eq!(
        on, output,
        "the window opened on {on}, not on {output}, under the pointer"
    );
    window
}

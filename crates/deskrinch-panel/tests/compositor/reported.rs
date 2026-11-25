//! Where the compositor's window management says the test's windows are,
//! waited for before a test judges the panel's bars by them.
//!
//! The panel places a window on a bar by the outputs the compositor's
//! window management reports for it. A test that checks only its own
//! client's `wl_surface.enter` (`TestWindow::wait_output`) cannot tell a
//! compositor that never reported a window's output there (sway 1.11 has
//! dropped one) from a panel that lost the report. Waiting here first, on a
//! window-management connection of the test's own, makes a bar that is
//! wrong afterwards the panel's fault alone.

use std::collections::BTreeSet;
use std::fmt;

use deskrinch_test_wait::wait_for;
use deskrinch_window_management::{Window, WindowManagement};

/// A window the compositor has not reported where the test expects it.
struct Unreported {
    app_id: String,
    output: String,
    /// The outputs it was reported on; `None` when it was not in the list.
    on: Option<BTreeSet<String>>,
}

/// Every window the compositor has not yet reported on its output.
struct Missing(Vec<Unreported>);

impl fmt::Debug for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for window in &self.0 {
            write!(
                f,
                "\nthe compositor never reported window {} on output {}",
                window.app_id, window.output
            )?;
            match &window.on {
                None => write!(f, " (it is not in its window list)")?,
                Some(on) if on.is_empty() => write!(f, " (it reported it on no output)")?,
                Some(on) => write!(f, " (it reported it on {on:?})")?,
            }
        }
        Ok(())
    }
}

/// Which of `expected`, as (app id, output), `windows` does not show on
/// that output.
fn missing(windows: &[Window], expected: &[(String, String)]) -> Missing {
    Missing(
        expected
            .iter()
            .filter_map(|(app_id, output)| {
                let on = windows
                    .iter()
                    .find(|window| &window.app_id == app_id)
                    .map(|window| window.outputs.clone());
                let there = on.as_ref().is_some_and(|on| on.contains(output));
                (!there).then(|| Unreported {
                    app_id: app_id.clone(),
                    output: output.clone(),
                    on,
                })
            })
            .collect(),
    )
}

/// Waits until the compositor's window management reports each window of
/// `expected`, given as (app id, output), on that output. Each window is
/// found by its app id, which must be its own.
///
/// # Panics
///
/// When two of `expected` share an app id, when the compositor's window
/// management does not report outputs, and when the wait runs out, naming
/// each window it never reported on its output and where it did report it.
pub(crate) fn wait_reported(expected: &[(String, String)]) {
    let ids: BTreeSet<&String> = expected.iter().map(|(app_id, _)| app_id).collect();
    assert_eq!(
        ids.len(),
        expected.len(),
        "the windows waited for share app ids, which find them"
    );
    let manager = deskrinch_window_management::connect(|_| {})
        .expect("connect to the compositor's window management");
    assert!(
        manager.capabilities().outputs,
        "the compositor's window management reports no outputs"
    );
    wait_for(
        "the compositor's window management to report every window on its output",
        || {
            let missing = missing(&manager.windows(), expected);
            if missing.0.is_empty() {
                Ok(())
            } else {
                Err(missing)
            }
        },
    );
}

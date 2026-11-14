//! The client's one registry: what comes and goes after attaching reaches
//! every attached state.

use std::sync::{Arc, Mutex};

use deskrinch_test_client::set_output_enabled;
use deskrinch_wayland_client::Client;

use deskrinch_test_wait::eventually;

use crate::support::{Record, attach, bounded, nested};

/// Turns an output back on when dropped, whatever the test did meanwhile:
/// the session's later tests expect both outputs.
struct Off(String);

impl Off {
    fn new(name: &str) -> Self {
        set_output_enabled(name, false).expect("turn the output off");
        Self(name.to_owned())
    }
}

impl Drop for Off {
    fn drop(&mut self) {
        // A failure here is reported by the test that follows.
        drop(set_output_enabled(&self.0, true));
    }
}

/// The `wl_output` globals `record` has seen added since setup.
fn outputs_added(record: &Mutex<Record>) -> Vec<u32> {
    record
        .lock()
        .unwrap()
        .globals_added
        .iter()
        .filter(|(_, interface)| interface == "wl_output")
        .map(|&(name, _)| name)
        .collect()
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_global_that_goes_and_comes_back_reaches_every_state() {
    nested();
    let client = Client::connect().unwrap();
    let first = attach(&client);
    let second = attach(&client);
    let records: [&Arc<Mutex<Record>>; 2] = [&first.record, &second.record];
    let (name, globals) = {
        let record = first.record.lock().unwrap();
        assert_eq!(record.output_globals.len(), 2, "{record:?}");
        (
            record.output_names.last().cloned().expect("named outputs"),
            record.output_globals.clone(),
        )
    };
    let off = Off::new(&name);
    for record in records {
        eventually("an output's global gone, for every state", || {
            record
                .lock()
                .unwrap()
                .globals_removed
                .iter()
                .any(|removed| globals.contains(removed))
        });
        assert_eq!(outputs_added(record), Vec::<u32>::new());
    }
    drop(off);
    for record in records {
        eventually("an output's global back, for every state", || {
            !outputs_added(record).is_empty()
        });
        let record = record.lock().unwrap();
        assert_eq!(record.globals_removed.len(), 1, "{record:?}");
        assert_eq!(record.ends, Vec::<String>::new());
    }
    // Both states heard the same events.
    assert_eq!(outputs_added(&first.record), outputs_added(&second.record));
    bounded("detaching the states", move || {
        drop(first);
        drop(second);
    });
}

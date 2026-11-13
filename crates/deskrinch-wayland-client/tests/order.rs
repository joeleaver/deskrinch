//! The registry's events reach a state after its own queue's: an object's
//! events sent before its global's removal are heard before the removal,
//! even when they arrive together. Against a stand-in compositor
//! ([`fake`]), which sends them in one write, as no real one can be made
//! to on cue.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#![cfg(test)]

#[expect(
    dead_code,
    reason = "each test binary uses its own part of the stand-in"
)]
mod fake;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_wayland_client::{
    Client, EndCause, Ended, Globals, OutputGlobal, Outputs, OutputsHandler,
};
use wayland_client::protocol::wl_output::WlOutput;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::ZxdgOutputV1;

use fake::{Arg, Fake, message};

/// How long anything here waits.
const PATIENCE: Duration = Duration::from_secs(30);
/// The output's global name, as the stand-in lists it.
const OUTPUT: u32 = 1;
/// `wl_registry.global_remove`, and `wl_output`'s `done` and `name`.
const GLOBAL_REMOVE: u16 = 1;
const OUTPUT_DONE: u16 = 2;
const OUTPUT_NAME: u16 = 4;

/// A state that binds the outputs and notes, in order, what it hears.
struct Listener {
    outputs: Outputs,
    heard: Arc<Mutex<Vec<String>>>,
}

impl Listener {
    fn hear(&self, what: String) {
        self.heard.lock().unwrap().push(what);
    }
}

impl Ended for Listener {
    fn end(&mut self, cause: EndCause) {
        self.hear(format!("ended {cause:?}"));
    }
}

impl Globals for Listener {
    fn global_removed(&mut self, name: u32) {
        if self.outputs.remove(name) {
            self.hear(format!("removed {name}"));
        }
    }
}

impl OutputsHandler for Listener {
    fn outputs(&mut self) -> &mut Outputs {
        &mut self.outputs
    }

    fn outputs_changed(&mut self) {
        let names: Vec<String> = self.outputs.all_names().into_iter().collect();
        self.hear(format!("named {}", names.join(" ")));
    }
}

wayland_client::delegate_dispatch!(Listener: [WlOutput: OutputGlobal] => Outputs);
wayland_client::delegate_dispatch!(Listener: [ZxdgOutputV1: OutputGlobal] => Outputs);
wayland_client::delegate_dispatch!(Listener: [ZxdgOutputManagerV1: ()] => Outputs);

#[test]
fn an_objects_events_come_before_its_globals_removal() {
    let (connection, fake) = Fake::start(&[(OUTPUT, "wl_output", 4)]);
    let client = Client::from_connection(connection).unwrap();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let listener = Listener {
        outputs: Outputs::default(),
        heard: Arc::clone(&heard),
    };
    let (attachment, ()) = client
        .attach(move |setup| {
            let mut listener = listener;
            let registry = setup.globals().registry().clone();
            listener.outputs.add(&registry, OUTPUT, 4, &setup.handle());
            setup.roundtrip(&mut listener)?;
            Ok::<_, wayland_client::DispatchError>((listener, ()))
        })
        .unwrap();
    let output = fake.bound(OUTPUT).expect("the output bound at setup");
    let registry = fake.registry().expect("the client's registry");

    // The output's name and `done`, then the global's removal, in one write.
    let mut burst = message(output, OUTPUT_NAME, &[Arg::Str("FAKE-1")]);
    burst.extend(message(output, OUTPUT_DONE, &[]));
    burst.extend(message(registry, GLOBAL_REMOVE, &[Arg::Uint(OUTPUT)]));
    fake.send(burst);

    let deadline = Instant::now() + PATIENCE;
    while !heard
        .lock()
        .unwrap()
        .iter()
        .any(|what| what.starts_with("removed"))
    {
        assert!(
            Instant::now() < deadline,
            "heard {:?}",
            heard.lock().unwrap()
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        *heard.lock().unwrap(),
        ["named FAKE-1".to_owned(), format!("removed {OUTPUT}")]
    );
    drop(attachment);
}

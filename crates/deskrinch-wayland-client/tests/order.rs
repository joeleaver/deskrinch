//! A state and the client's registry, against a stand-in compositor
//! ([`fake`]) that does on cue what no real one can be made to:
//!
//! - the registry's events reach a state after its own queue's: an
//!   object's events sent before its global's removal are heard before the
//!   removal, even when they arrive together, in one write;
//! - an output handed to a state twice (in its setup's list, and again to
//!   [`Globals::global_added`]) is bound once;
//! - a detach's requests leave in the round that releases the state, with
//!   nothing else to send them.

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
    Client, EndCause, Globals, Lifecycle, OutputGlobal, Outputs, OutputsHandler,
};
use wayland_client::QueueHandle;
use wayland_client::globals::Global;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::WlRegistry;
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
    /// Whether its release let go of everything.
    released: bool,
}

impl Listener {
    fn hear(&self, what: String) {
        self.heard.lock().unwrap().push(what);
    }
}

impl Lifecycle for Listener {
    fn end(&mut self, cause: EndCause) {
        self.hear(format!("ended {cause:?}"));
    }

    fn release(&mut self) {
        self.released = self.outputs.release();
    }

    fn released(&self) -> bool {
        self.released
    }
}

impl Globals for Listener {
    fn global_added(&mut self, registry: &WlRegistry, global: &Global, qh: &QueueHandle<Self>) {
        if global.interface == "wl_output" {
            self.outputs.add(registry, global.name, global.version, qh);
        }
    }

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
        released: false,
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
    let [output] = fake.bound(OUTPUT)[..] else {
        panic!("the output bound once at setup: {:?}", fake.bound(OUTPUT));
    };
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

#[test]
fn an_output_handed_over_twice_is_bound_once() {
    let (connection, fake) = Fake::start(&[(OUTPUT, "wl_output", 4)]);
    let client = Client::from_connection(connection).unwrap();
    let listener = Listener {
        outputs: Outputs::default(),
        heard: Arc::default(),
        released: false,
    };
    let (attachment, ()) = client
        .attach(move |setup| {
            let mut listener = listener;
            let registry = setup.globals().registry().clone();
            let qh = setup.handle();
            // From the setup's list, then as the client's thread would hand
            // it over had it arrived during the setup's exchanges.
            let global = Global {
                name: OUTPUT,
                interface: "wl_output".to_owned(),
                version: 4,
            };
            listener.outputs.add(&registry, OUTPUT, 4, &qh);
            listener.global_added(&registry, &global, &qh);
            setup.roundtrip(&mut listener)?;
            Ok::<_, wayland_client::DispatchError>((listener, ()))
        })
        .unwrap();
    assert_eq!(fake.bound(OUTPUT).len(), 1, "{:?}", fake.bound(OUTPUT));
    drop(attachment);
}

/// `wl_output.release`.
const OUTPUT_RELEASE: u32 = 0;

#[test]
fn a_detachs_requests_leave_in_its_own_round() {
    let (connection, fake) = Fake::start(&[(OUTPUT, "wl_output", 4)]);
    let client = Client::from_connection(connection).unwrap();
    let listener = Listener {
        outputs: Outputs::default(),
        heard: Arc::default(),
        released: false,
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
    let [output] = fake.bound(OUTPUT)[..] else {
        panic!("the output bound once: {:?}", fake.bound(OUTPUT));
    };
    assert_eq!(fake.requests_to(output), Vec::<u32>::new());
    // Nothing is sent after this: only the detach's own round can flush
    // the output's release.
    drop(attachment);
    let deadline = Instant::now() + PATIENCE;
    while fake.requests_to(output).is_empty() {
        assert!(Instant::now() < deadline, "the output's release never left");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fake.requests_to(output), [OUTPUT_RELEASE]);
    // It let go of everything: dropped at once, not releasing.
    let states = client.states();
    assert_eq!((states.attached, states.releasing), (0, 0), "{states:?}");
}

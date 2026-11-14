//! A stand-in for a `DBusActivatable` application, served in this process
//! or started by `dbus-daemon` through service activation.
//!
//! For activation, the bus is given a service file whose `Exec` runs this
//! test binary with only [`activation_stand_in`] selected. That test does
//! nothing unless `dbus-daemon` started it (it then has
//! `DBUS_STARTER_ADDRESS`): it serves the application until told to quit.

use std::collections::HashMap;
use std::fs;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use deskrinch_desktop_entries::{Launched, Target, launch};
use zbus::OwnedVariant;

use crate::support::{PrivateBus, entry, serve_all};
use deskrinch_test_wait::PATIENCE;

/// A stand-in for a `DBusActivatable` application.
pub(crate) struct FakeApplication {
    pub(crate) calls: Arc<Mutex<Vec<AppCall>>>,
}

/// A call the stand-in application received.
#[derive(Debug)]
pub(crate) struct AppCall {
    method: &'static str,
    /// The URIs of `Open`, or the action of `ActivateAction`.
    args: Vec<String>,
    parameters: Vec<OwnedVariant>,
    platform_data: HashMap<String, OwnedVariant>,
}

impl AppCall {
    /// The call as text, checking that both token keys agree.
    pub(crate) fn describe(&self) -> String {
        let get = |key: &str| {
            self.platform_data
                .get(key)
                .map(|value| String::try_from(value.clone()).unwrap())
        };
        assert_eq!(get("activation-token"), get("desktop-startup-id"));
        format!(
            "{} {:?} parameters={} token={}",
            self.method,
            self.args,
            self.parameters.len(),
            get("activation-token").unwrap_or_default()
        )
    }
}

impl FakeApplication {
    fn record(&self, call: AppCall) {
        self.calls.lock().unwrap().push(call);
    }
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl FakeApplication {
    fn activate(&self, platform_data: HashMap<String, OwnedVariant>) {
        self.record(AppCall {
            method: "Activate",
            args: Vec::new(),
            parameters: Vec::new(),
            platform_data,
        });
    }

    fn open(&self, uris: Vec<String>, platform_data: HashMap<String, OwnedVariant>) {
        self.record(AppCall {
            method: "Open",
            args: uris,
            parameters: Vec::new(),
            platform_data,
        });
    }

    fn activate_action(
        &self,
        action: String,
        parameter: Vec<OwnedVariant>,
        platform_data: HashMap<String, OwnedVariant>,
    ) {
        self.record(AppCall {
            method: "ActivateAction",
            args: vec![action],
            parameters: parameter,
            platform_data,
        });
    }
}

/// The activated stand-in's bus name and object.
const ACTIVATED: &str = "org.deskrinch.Activated";
const ACTIVATED_PATH: &str = "/org/deskrinch/Activated";
/// The interface the test reads the activated stand-in's calls through.
const PROBE: &str = "org.deskrinch.Test.Probe";

/// Lets the test ask the activated stand-in, in its own process, what it
/// received, and tell it to quit.
struct Probe {
    calls: Arc<Mutex<Vec<AppCall>>>,
    quit: mpsc::Sender<()>,
}

#[zbus::interface(name = "org.deskrinch.Test.Probe")]
impl Probe {
    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(AppCall::describe)
            .collect()
    }

    fn quit(&self) {
        self.quit.send(()).ok();
    }
}

#[test]
#[ignore = "run by dbus-daemon when it activates the stand-in"]
fn activation_stand_in() {
    let Ok(address) = std::env::var("DBUS_STARTER_ADDRESS") else {
        return;
    };
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (quit, quitting) = mpsc::channel();
    let application = FakeApplication {
        calls: Arc::clone(&calls),
    };
    // Both objects before the name: the test calls `Probe` as soon as it
    // sees the name owned.
    let _connection = serve_all(&address, ACTIVATED, |builder| {
        builder
            .serve_at(ACTIVATED_PATH, application)
            .serve_at(ACTIVATED_PATH, Probe { calls, quit })
    });
    quitting.recv_timeout(PATIENCE * 3).ok();
}

#[test]
fn a_dbus_activatable_entry_is_started_by_service_activation() {
    let services = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();
    fs::write(
        services.path().join(format!("{ACTIVATED}.service")),
        // Through `sh` to give the test harness a standard output: the
        // daemon's own is a pipe nobody reads once it has printed its
        // address.
        format!(
            "[D-BUS Service]\nName={ACTIVATED}\n\
             Exec=/bin/sh -c 'exec \"$0\" --exact application::activation_stand_in \
             --ignored --quiet >/dev/null' {}\n",
            exe.display()
        ),
    )
    .unwrap();
    let bus = PrivateBus::with_services(&[services.path()]);
    let dir = tempfile::tempdir().unwrap();
    // Exec would fail: only activation can start it.
    let app = entry(
        dir.path(),
        &format!("{ACTIVATED}.desktop"),
        "[Desktop Entry]\nType=Application\nName=Activated\nDBusActivatable=true\n\
         Exec=/nonexistent/program\n",
    );
    let client = zbus::block_on(zbus::connection::Builder::address(bus.address()).build()).unwrap();
    let owned = |client: &zbus::Connection| -> bool {
        let reply = zbus::block_on(client.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &ACTIVATED,
        ))
        .unwrap();
        reply.body().deserialize().unwrap()
    };
    assert!(!owned(&client), "nothing runs before the launch");

    let env = deskrinch_desktop_entries::Environment {
        session_bus: Some(bus.address().to_owned()),
        ..Default::default()
    };
    let context = bus
        .isolate(deskrinch_desktop_entries::LaunchContext::new(&env).with_activation_token("tok"));
    let launched = launch(&app, None, &[Target::Path("/x".into())], &context).unwrap();
    assert_eq!(
        launched,
        [Launched::Activated {
            bus_name: ACTIVATED.into()
        }]
    );
    assert!(owned(&client), "the bus started the stand-in");

    let calls = zbus::block_on(client.call_method(
        Some(ACTIVATED),
        ACTIVATED_PATH,
        Some(PROBE),
        "Calls",
        &(),
    ))
    .unwrap();
    let calls: Vec<String> = calls.body().deserialize().unwrap();
    assert_eq!(calls, [r#"Open ["file:///x"] parameters=0 token=tok"#]);
    let pid = zbus::block_on(client.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "GetConnectionUnixProcessID",
        &ACTIVATED,
    ))
    .unwrap();
    let pid: u32 = pid.body().deserialize().unwrap();
    assert_ne!(pid, std::process::id(), "a process of its own");
    // Expecting no reply: the stand-in exits as soon as it is told to, and
    // may disconnect before a reply would have been sent.
    let quit = zbus::Message::method_call(ACTIVATED_PATH, "Quit")
        .destination(ACTIVATED)
        .interface(PROBE)
        .with_flags(zbus::message::Flags::NO_REPLY_EXPECTED)
        .build(&())
        .unwrap();
    zbus::block_on(client.send(&quit)).unwrap();
}

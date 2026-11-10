//! Systemd scopes and D-Bus activation, against stand-ins on a private bus.
//!
//! The stand-in systemd checks what a real one is sent and answers as one
//! does; it cannot move a process between cgroups, so these tests cover
//! the protocol and the launch's ordering, not the kernel side.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use deskrinch_desktop_entries::{
    DesktopEntry, DirectReason, Environment, LaunchContext, LaunchError, Launched, Placement,
    Target, launch,
};
use zbus::object_server::SignalEmitter;
use zbus::{ObjectPath, OwnedObjectPath, OwnedVariant};

use crate::application::{AppCall, FakeApplication};
use crate::support::{PrivateBus, entry, recorded, recorder};

/// What the stand-in systemd was asked.
#[derive(Debug, Clone)]
struct ScopeCall {
    unit: String,
    mode: String,
    pids: Vec<u32>,
    slice: String,
    collect_mode: String,
    /// The program the PID ran when the call arrived.
    exe: PathBuf,
}

struct FakeSystemd {
    result: &'static str,
    calls: Arc<Mutex<Vec<ScopeCall>>>,
}

fn string(properties: &[(String, OwnedVariant)], key: &str) -> String {
    let (_, value) = properties.iter().find(|(k, _)| k == key).unwrap();
    String::try_from(value.clone()).unwrap()
}

#[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
impl FakeSystemd {
    async fn start_transient_unit(
        &self,
        name: String,
        mode: String,
        properties: Vec<(String, OwnedVariant)>,
        auxiliary: Vec<(String, Vec<(String, OwnedVariant)>)>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        assert_eq!(auxiliary.len(), 0, "no auxiliary units");
        let (_, pids) = properties.iter().find(|(k, _)| k == "PIDs").unwrap();
        let pids = Vec::<u32>::try_from(pids.clone()).unwrap();
        let exe = std::fs::read_link(format!("/proc/{}/exe", pids[0])).unwrap();
        self.calls.lock().unwrap().push(ScopeCall {
            unit: name.clone(),
            mode,
            pids,
            slice: string(&properties, "Slice"),
            collect_mode: string(&properties, "CollectMode"),
            exe,
        });
        let job = ObjectPath::try_from("/org/freedesktop/systemd1/job/7").unwrap();
        // Sent before the reply: the launch must not miss it.
        Self::job_removed(&emitter, 7, job.clone(), &name, self.result).await?;
        Ok(job.into())
    }

    #[zbus(signal)]
    async fn job_removed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        job: ObjectPath<'_>,
        unit: &str,
        result: &str,
    ) -> zbus::Result<()>;
}

/// A bus with a stand-in systemd that ends jobs with `result`.
fn systemd(result: &'static str) -> (PrivateBus, zbus::Connection, Arc<Mutex<Vec<ScopeCall>>>) {
    let bus = PrivateBus::start();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let fake = FakeSystemd {
        result,
        calls: Arc::clone(&calls),
    };
    let connection = bus.serve(
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        fake,
    );
    (bus, connection, calls)
}

fn context(bus: &PrivateBus, out: &Path) -> LaunchContext {
    let env = Environment {
        session_bus: Some(bus.address().to_owned()),
        ..Environment::default()
    };
    bus.isolate(LaunchContext::new(&env).with_env("DESKRINCH_TEST_OUT", out))
}

fn recorder_entry(dir: &Path) -> DesktopEntry {
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=Recorder\nExec={}\n",
        recorder(dir).display()
    );
    entry(dir, "org.deskrinch.test-recorder.desktop", &text)
}

fn process(launched: &[Launched]) -> (u32, Placement) {
    match launched {
        [Launched::Process { pid, placement }] => (*pid, placement.clone()),
        other => panic!("one process expected, got {other:?}"),
    }
}

#[test]
fn places_the_process_in_a_scope_before_it_execs() {
    let (bus, _systemd, calls) = systemd("done");
    let dir = tempfile::tempdir().unwrap();
    let context = context(&bus, dir.path()).with_scope_launcher("deskrinch");
    let launched = launch(&recorder_entry(dir.path()), None, &[], &context).unwrap();
    let (pid, placement) = process(&launched);

    let Placement::Scope { unit } = placement else {
        panic!("not in a scope: {placement:?}");
    };
    assert!(
        unit.starts_with(r"app-deskrinch-org.deskrinch.test\x2drecorder-"),
        "{unit}"
    );
    assert_eq!(
        Path::new(&unit).extension(),
        Some("scope".as_ref()),
        "{unit}"
    );
    let calls = calls.lock().unwrap().clone();
    let [call] = &calls[..] else {
        panic!("one scope expected: {calls:?}");
    };
    assert_eq!(call.unit, unit);
    assert_eq!(call.mode, "fail");
    assert_eq!(call.pids, [pid]);
    assert_eq!(call.slice, "app.slice");
    assert_eq!(call.collect_mode, "inactive-or-failed");
    // When systemd was asked, the process had not exec'd the recorder yet.
    assert_eq!(call.exe, std::env::current_exe().unwrap());
    // And once the scope was there, it ran.
    assert!(recorded(dir.path(), pid).contains_key("cwd"));
}

#[test]
fn a_failed_scope_still_runs_the_process() {
    let (bus, _systemd, _calls) = systemd("failed");
    let dir = tempfile::tempdir().unwrap();
    let launched = launch(
        &recorder_entry(dir.path()),
        None,
        &[],
        &context(&bus, dir.path()),
    )
    .unwrap();
    let (pid, placement) = process(&launched);
    assert!(
        matches!(&placement, Placement::Direct(DirectReason::ScopeFailed(why)) if why.contains("failed")),
        "{placement:?}"
    );
    assert!(recorded(dir.path(), pid).contains_key("cwd"));
}

#[test]
fn without_systemd_or_with_scopes_off_the_process_runs_directly() {
    let bus = PrivateBus::start();
    let dir = tempfile::tempdir().unwrap();
    let app = recorder_entry(dir.path());
    let launched = launch(&app, None, &[], &context(&bus, dir.path())).unwrap();
    let (pid, placement) = process(&launched);
    assert_eq!(placement, Placement::Direct(DirectReason::NoSystemd));
    recorded(dir.path(), pid);

    let (bus, _systemd, calls) = systemd("done");
    let off = context(&bus, dir.path()).without_scopes();
    let launched = launch(&app, None, &[], &off).unwrap();
    let (pid, placement) = process(&launched);
    assert_eq!(placement, Placement::Direct(DirectReason::Disabled));
    assert!(calls.lock().unwrap().is_empty());
    recorded(dir.path(), pid);
}

#[test]
fn activates_dbus_activatable_entries_over_dbus() {
    let bus = PrivateBus::start();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let application = FakeApplication {
        calls: Arc::clone(&calls),
    };
    let _served = bus.serve(
        "org.deskrinch.Test-App",
        "/org/deskrinch/Test_App",
        application,
    );
    let dir = tempfile::tempdir().unwrap();
    // Exec would fail: D-Bus activation must not fall back to it.
    let app = entry(
        dir.path(),
        "org.deskrinch.Test-App.desktop",
        "[Desktop Entry]\nType=Application\nName=T\nDBusActivatable=true\n\
         Exec=/nonexistent/program\nActions=compose;\n\
         [Desktop Action compose]\nName=Compose\n",
    );
    let context = context(&bus, dir.path()).with_activation_token("tok");
    let activated = [Launched::Activated {
        bus_name: "org.deskrinch.Test-App".into(),
    }];
    assert_eq!(launch(&app, None, &[], &context).unwrap(), activated);
    let targets = [
        Target::Path("/a b".into()),
        Target::Uri("https://e.org".into()),
    ];
    assert_eq!(launch(&app, None, &targets, &context).unwrap(), activated);
    assert_eq!(
        launch(&app, Some("compose"), &[], &context).unwrap(),
        activated
    );
    let calls: Vec<_> = calls
        .lock()
        .unwrap()
        .iter()
        .map(AppCall::describe)
        .collect();
    assert_eq!(
        calls,
        [
            "Activate [] parameters=0 token=tok",
            r#"Open ["file:///a%20b", "https://e.org"] parameters=0 token=tok"#,
            r#"ActivateAction ["compose"] parameters=0 token=tok"#,
        ]
    );
}

#[test]
fn reports_a_failed_activation() {
    let bus = PrivateBus::start();
    let dir = tempfile::tempdir().unwrap();
    let app = entry(
        dir.path(),
        "org.deskrinch.Absent.desktop",
        "[Desktop Entry]\nType=Application\nName=A\nDBusActivatable=true\nExec=true\n",
    );
    let error = launch(&app, None, &[], &context(&bus, dir.path())).unwrap_err();
    assert!(
        matches!(&error, LaunchError::Activation { bus_name, .. } if bus_name == "org.deskrinch.Absent"),
        "{error:?}"
    );
    let invalid = entry(
        dir.path(),
        "not-a-bus-name.desktop",
        "[Desktop Entry]\nType=Application\nName=A\nDBusActivatable=true\n",
    );
    assert!(matches!(
        launch(&invalid, None, &[], &context(&bus, dir.path())),
        Err(LaunchError::InvalidBusName(_))
    ));
}

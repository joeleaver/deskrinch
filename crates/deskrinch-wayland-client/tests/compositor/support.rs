//! A small state of the tests' own, attached the way the shell's crates
//! attach theirs, and bounded waits.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_test_client::Compositor;
use deskrinch_wayland_client::{
    Attachment, Client, Detached, EndCause, Ended, OutputGlobal, Outputs, OutputsHandler,
};
use wayland_client::globals::GlobalListContents;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::ZxdgOutputV1;

/// How long anything a test waits for gets: a hang fails instead of
/// blocking.
pub(crate) const PATIENCE: Duration = Duration::from_secs(60);

/// Refuses to run outside a nested session.
pub(crate) fn nested() {
    let _compositor = Compositor::current();
}

/// What a test learns from its state: the outputs it saw named at setup,
/// and how it ended.
#[derive(Debug, Default)]
pub(crate) struct Record {
    pub(crate) outputs: usize,
    pub(crate) ends: Vec<String>,
}

/// What a callback asks of the state when it is answered.
#[derive(Debug, Clone)]
pub(crate) enum OnDone {
    /// Nothing.
    Nothing,
    /// Panic.
    Panic,
    /// Drop the attachment held here, from inside the handler.
    Detach(Arc<Mutex<Option<Attachment>>>),
}

/// The tests' state: the outputs, and a record shared with the test.
#[derive(Debug)]
pub(crate) struct Probe {
    outputs: Outputs,
    record: Arc<Mutex<Record>>,
    detached: Detached,
}

impl Probe {
    fn record(&self) -> std::sync::MutexGuard<'_, Record> {
        self.record.lock().unwrap()
    }
}

impl Ended for Probe {
    fn end(&mut self, cause: EndCause) {
        self.record().ends.push(format!("{cause:?}"));
    }
}

impl OutputsHandler for Probe {
    fn outputs(&mut self) -> &mut Outputs {
        &mut self.outputs
    }

    fn outputs_changed(&mut self) {}
}

wayland_client::delegate_dispatch!(Probe: [WlOutput: OutputGlobal] => Outputs);
wayland_client::delegate_dispatch!(Probe: [ZxdgOutputV1: OutputGlobal] => Outputs);
wayland_client::delegate_dispatch!(Probe: [ZxdgOutputManagerV1: ()] => Outputs);

impl Dispatch<WlRegistry, GlobalListContents> for Probe {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlCallback, OnDone> for Probe {
    fn event(
        _: &mut Self,
        _: &WlCallback,
        _: wl_callback::Event,
        on_done: &OnDone,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match on_done {
            OnDone::Nothing => {}
            OnDone::Panic => panic!("a test handler panics, as asked"),
            OnDone::Detach(attachment) => drop(attachment.lock().unwrap().take()),
        }
    }
}

/// An attached [`Probe`]: its handle, its queue's handle, and its record.
pub(crate) struct Attached {
    pub(crate) attachment: Attachment,
    pub(crate) queue: QueueHandle<Probe>,
    pub(crate) record: Arc<Mutex<Record>>,
}

/// Attaches a [`Probe`] to `client`, its outputs bound and named in two
/// exchanges at setup; panics if that takes longer than [`PATIENCE`].
pub(crate) fn attach(client: &Client) -> Attached {
    let client = client.clone();
    bounded("attaching a probe", move || {
        let record = Arc::new(Mutex::new(Record::default()));
        let setup_record = Arc::clone(&record);
        let (attachment, queue) = client
            .attach(move |setup| {
                let mut probe = Probe {
                    outputs: Outputs::default(),
                    record: setup_record,
                    detached: setup.detached(),
                };
                let handle = setup.handle();
                let registry = setup.globals().registry().clone();
                let globals = setup.globals().contents().clone_list();
                for global in &globals {
                    if global.interface == ZxdgOutputManagerV1::interface().name {
                        probe
                            .outputs
                            .add_manager(&registry, global.name, global.version, &handle);
                    }
                }
                for global in &globals {
                    if global.interface == WlOutput::interface().name {
                        probe
                            .outputs
                            .add(&registry, global.name, global.version, &handle);
                    }
                }
                setup.roundtrip(&mut probe)?;
                setup.roundtrip(&mut probe)?;
                let outputs = probe.outputs.all_names().len();
                probe.record.lock().unwrap().outputs = outputs;
                assert!(!probe.detached.is_set());
                Ok::<_, wayland_client::DispatchError>((probe, handle))
            })
            .expect("attach a probe");
        Attached {
            attachment,
            queue,
            record,
        }
    })
}

/// Runs `work` on a thread of its own and waits for it at most
/// [`PATIENCE`]: a hang fails the test instead of blocking it.
pub(crate) fn bounded<T: Send + 'static>(
    what: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        // The receiver may have given up; nothing to tell then.
        drop(done.send(work()));
    });
    finished
        .recv_timeout(PATIENCE)
        .unwrap_or_else(|error| panic!("{what}: {error} (a hang)"))
}

/// Waits until `check` holds, at most [`PATIENCE`].
pub(crate) fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !check() {
        assert!(Instant::now() < deadline, "{what}: not within {PATIENCE:?}");
        thread::sleep(Duration::from_millis(20));
    }
}

/// The ids of this process's threads that are a client's (named
/// `deskrinch-wl`), as `/proc/self/task` lists them now.
pub(crate) fn client_threads() -> std::collections::BTreeSet<u32> {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(Result::ok)
        .filter(|task| {
            std::fs::read_to_string(task.path().join("comm"))
                .is_ok_and(|name| name.trim() == "deskrinch-wl")
        })
        .filter_map(|task| task.file_name().to_str()?.parse().ok())
        .collect()
}

/// Whether the thread with id `id` is still in this process.
pub(crate) fn thread_alive(id: u32) -> bool {
    std::path::Path::new(&format!("/proc/self/task/{id}")).exists()
}

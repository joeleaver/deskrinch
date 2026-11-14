//! A small state of the tests' own, attached the way the shell's crates
//! attach theirs, and bounded waits.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_test_client::Compositor;
use deskrinch_wayland_client::{
    Attachment, Client, Detached, EndCause, Globals, Lifecycle, OutputGlobal, Outputs,
    OutputsHandler, Remaining,
};
use wayland_client::globals::Global;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::WlRegistry;
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

/// What a test learns from its state: the id of the thread its setup ran
/// on (the client's), the outputs it saw at setup (their names, and their
/// `wl_output` globals), the globals that came and went after it, how it
/// ended, and whether it was released; and what the test asks of its
/// release.
#[derive(Debug, Default)]
pub(crate) struct Record {
    /// Set by the test: the state's release leaves it releasing, as one
    /// holding an object without a destructor would be.
    pub(crate) linger: bool,
    pub(crate) releases: usize,
    pub(crate) thread: Option<u32>,
    pub(crate) outputs: usize,
    pub(crate) output_names: Vec<String>,
    pub(crate) output_globals: Vec<u32>,
    pub(crate) globals_added: Vec<(u32, String)>,
    pub(crate) globals_removed: Vec<u32>,
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
    remaining: Remaining,
}

impl Probe {
    fn record(&self) -> std::sync::MutexGuard<'_, Record> {
        self.record.lock().unwrap()
    }
}

impl Lifecycle for Probe {
    fn end(&mut self, cause: EndCause) {
        self.record().ends.push(format!("{cause:?}"));
    }

    fn release(&mut self) {
        if !self.outputs.release() {
            self.remaining.linger();
        }
        let linger = {
            let mut record = self.record();
            record.releases += 1;
            record.linger
        };
        if linger {
            self.remaining.linger();
        }
    }

    fn released(&self) -> bool {
        self.remaining.is_empty()
    }
}

impl Globals for Probe {
    fn global_added(&mut self, _: &WlRegistry, global: &Global, _: &QueueHandle<Self>) {
        self.record()
            .globals_added
            .push((global.name, global.interface.clone()));
    }

    fn global_removed(&mut self, name: u32) {
        self.record().globals_removed.push(name);
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
                    remaining: Remaining::default(),
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
                let names = probe.outputs.all_names();
                let mut record = probe.record();
                record.outputs = names.len();
                record.output_names = names.into_iter().collect();
                record.output_globals = globals
                    .iter()
                    .filter(|global| global.interface == WlOutput::interface().name)
                    .map(|global| global.name)
                    .collect();
                // Setup runs on the client's thread.
                record.thread = Some(
                    rustix::thread::gettid()
                        .as_raw_nonzero()
                        .get()
                        .unsigned_abs(),
                );
                drop(record);
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

/// Whether the thread with id `id` is still listed in this process's
/// `/proc/self/task`.
pub(crate) fn thread_alive(id: u32) -> bool {
    std::path::Path::new(&format!("/proc/self/task/{id}")).exists()
}

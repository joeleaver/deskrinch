//! A throwaway Wayland client with mapped `xdg_toplevel`s: the windows the
//! compositor tests look at and act on.
//!
//! A [`TestClient`] is its own connection, so the compositor sees a separate
//! client, dispatched on a thread of its own so that it answers pings and
//! configures like a live application: it acknowledges each configure and
//! draws a buffer of the size asked for. A close request is recorded and
//! obeyed: the window unmaps, as an application would.

use std::collections::HashMap;
use std::fs::File;
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

/// The size a window draws when the compositor leaves it the choice.
const DEFAULT_SIZE: (i32, i32) = (320, 240);
/// How long a window waits to be mapped.
const PATIENCE: Duration = Duration::from_secs(10);

/// What the client's thread reports about one window.
#[derive(Debug, Default)]
struct Status {
    /// The size the last toplevel configure asked for (0 = client's choice).
    asked: (i32, i32),
    /// The size of the buffer attached, once there is one.
    drawn: Option<(i32, i32)>,
    close_requested: bool,
}

/// One window, shared by the test and the client's thread.
#[derive(Debug)]
struct Shared {
    surface: WlSurface,
    status: Mutex<Status>,
    mapped: Condvar,
}

/// The client's thread's state.
#[derive(Debug)]
struct Client {
    shm: WlShm,
    /// Outputs by name, as `wl_output` names them.
    outputs: Arc<Mutex<HashMap<String, WlOutput>>>,
}

/// One Wayland client, which may have several windows.
#[derive(Debug)]
pub(crate) struct TestClient {
    connection: Connection,
    queue: QueueHandle<Client>,
    compositor: WlCompositor,
    wm_base: XdgWmBase,
    outputs: Arc<Mutex<HashMap<String, WlOutput>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TestClient {
    pub(crate) fn connect() -> Self {
        let connection = Connection::connect_to_env().expect("connect the test client");
        let (globals, mut queue) =
            registry_queue_init::<Client>(&connection).expect("list globals");
        let qh = queue.handle();
        let compositor: WlCompositor = globals.bind(&qh, 1..=4, ()).expect("wl_compositor");
        let shm: WlShm = globals.bind(&qh, 1..=1, ()).expect("wl_shm");
        let wm_base: XdgWmBase = globals.bind(&qh, 1..=1, ()).expect("xdg_wm_base");
        for global in globals.contents().clone_list() {
            if global.interface == WlOutput::interface().name {
                globals.registry().bind::<WlOutput, _, _>(
                    global.name,
                    global.version.min(4),
                    &qh,
                    (),
                );
            }
        }
        let outputs = Arc::new(Mutex::new(HashMap::new()));
        let mut client = Client {
            shm,
            outputs: Arc::clone(&outputs),
        };
        queue.roundtrip(&mut client).expect("learn the outputs");

        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let stop = Arc::clone(&stop);
            move || run(queue, client, &stop)
        });
        Self {
            connection,
            queue: qh,
            compositor,
            wm_base,
            outputs,
            stop,
            thread: Some(thread),
        }
    }

    /// The names of the compositor's outputs, sorted.
    pub(crate) fn output_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.outputs.lock().unwrap().keys().cloned().collect();
        names.sort();
        names
    }

    /// Opens a window and returns once it has drawn its first buffer.
    pub(crate) fn open(&self, title: &str, app_id: &str) -> TestWindow {
        self.open_with_parent(title, app_id, None)
    }

    /// Opens a window that belongs to `parent`, like a dialog.
    pub(crate) fn open_child(&self, parent: &TestWindow, title: &str, app_id: &str) -> TestWindow {
        self.open_with_parent(title, app_id, Some(&parent.toplevel))
    }

    fn open_with_parent(
        &self,
        title: &str,
        app_id: &str,
        parent: Option<&XdgToplevel>,
    ) -> TestWindow {
        let surface = self.compositor.create_surface(&self.queue, ());
        let shared = Arc::new(Shared {
            surface: surface.clone(),
            status: Mutex::new(Status::default()),
            mapped: Condvar::new(),
        });
        let xdg_surface = self
            .wm_base
            .get_xdg_surface(&surface, &self.queue, Arc::clone(&shared));
        let toplevel = xdg_surface.get_toplevel(&self.queue, Arc::clone(&shared));
        toplevel.set_title(title.to_owned());
        toplevel.set_app_id(app_id.to_owned());
        toplevel.set_parent(parent);
        surface.commit();
        self.flush();

        let status = shared.status.lock().unwrap();
        let (status, timeout) = shared
            .mapped
            .wait_timeout_while(status, PATIENCE, |status| status.drawn.is_none())
            .unwrap();
        assert!(
            !timeout.timed_out(),
            "the window {title:?} was never configured"
        );
        drop(status);
        TestWindow {
            connection: self.connection.clone(),
            outputs: Arc::clone(&self.outputs),
            toplevel,
            xdg_surface,
            shared,
        }
    }

    fn flush(&self) {
        self.connection.flush().expect("send to the compositor");
    }
}

impl Drop for TestClient {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.connection.display().sync(&self.queue, ());
        // The compositor may be gone already when a test fails or ends it.
        drop(self.connection.flush());
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

/// A mapped toplevel; dropping it destroys it.
#[derive(Debug)]
pub(crate) struct TestWindow {
    connection: Connection,
    outputs: Arc<Mutex<HashMap<String, WlOutput>>>,
    toplevel: XdgToplevel,
    xdg_surface: XdgSurface,
    shared: Arc<Shared>,
}

impl TestWindow {
    /// Changes the title.
    pub(crate) fn set_title(&self, title: &str) {
        self.toplevel.set_title(title.to_owned());
        self.commit();
    }

    /// Asks to be maximized, or no longer.
    pub(crate) fn set_maximized(&self, maximized: bool) {
        if maximized {
            self.toplevel.set_maximized();
        } else {
            self.toplevel.unset_maximized();
        }
        self.commit();
    }

    /// Asks to be fullscreen (on the named output, or one the compositor
    /// picks for `None`), or no longer fullscreen.
    pub(crate) fn set_fullscreen(&self, fullscreen: bool, output: Option<&str>) {
        if fullscreen {
            let outputs = self.outputs.lock().unwrap();
            let output = output.map(|name| outputs.get(name).expect("a known output"));
            self.toplevel.set_fullscreen(output);
        } else {
            self.toplevel.unset_fullscreen();
        }
        self.commit();
    }

    /// Whether the compositor has asked this window to close.
    pub(crate) fn close_requested(&self) -> bool {
        self.shared.status.lock().unwrap().close_requested
    }

    fn commit(&self) {
        self.shared.surface.commit();
        self.connection.flush().expect("send to the compositor");
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        self.toplevel.destroy();
        self.xdg_surface.destroy();
        self.shared.surface.destroy();
        drop(self.connection.flush());
    }
}

fn run(mut queue: EventQueue<Client>, mut client: Client, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        if queue.blocking_dispatch(&mut client).is_err() {
            return;
        }
    }
}

impl Client {
    /// A buffer of transparent pixels of `size`, in a file of its own.
    fn buffer(&self, (width, height): (i32, i32), qh: &QueueHandle<Self>) -> WlBuffer {
        let stride = width * 4;
        let size = stride * height;
        let file: File = tempfile::tempfile().expect("buffer file");
        file.set_len(u64::try_from(size).unwrap())
            .expect("size the buffer");
        let pool = self.shm.create_pool(file.as_fd(), size, qh, ());
        let buffer = pool.create_buffer(0, width, height, stride, wl_shm::Format::Argb8888, qh, ());
        pool.destroy();
        buffer
    }
}

impl Dispatch<XdgToplevel, Arc<Shared>> for Client {
    fn event(
        _: &mut Self,
        _: &XdgToplevel,
        event: xdg_toplevel::Event,
        shared: &Arc<Shared>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } => {
                shared.status.lock().unwrap().asked = (width, height);
            }
            xdg_toplevel::Event::Close => {
                shared.status.lock().unwrap().close_requested = true;
                // Obey, as an application would: unmap.
                shared.surface.attach(None, 0, 0);
                shared.surface.commit();
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgSurface, Arc<Shared>> for Client {
    fn event(
        client: &mut Self,
        xdg_surface: &XdgSurface,
        event: xdg_surface::Event,
        shared: &Arc<Shared>,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let xdg_surface::Event::Configure { serial } = event else {
            return;
        };
        xdg_surface.ack_configure(serial);
        let mut status = shared.status.lock().unwrap();
        if status.close_requested {
            return;
        }
        let size = match status.asked {
            (width, height) if width > 0 && height > 0 => (width, height),
            _ => status.drawn.unwrap_or(DEFAULT_SIZE),
        };
        if status.drawn != Some(size) {
            let buffer = client.buffer(size, qh);
            shared.surface.attach(Some(&buffer), 0, 0);
            shared.surface.damage(0, 0, size.0, size.1);
            status.drawn = Some(size);
        }
        shared.surface.commit();
        shared.mapped.notify_all();
    }
}

impl Dispatch<WlOutput, ()> for Client {
    fn event(
        client: &mut Self,
        output: &WlOutput,
        event: wl_output::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            client.outputs.lock().unwrap().insert(name, output.clone());
        }
    }
}

impl Dispatch<WlBuffer, ()> for Client {
    fn event(
        _: &mut Self,
        buffer: &WlBuffer,
        event: wl_buffer::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Each buffer is attached once; when the compositor lets go of it,
        // it is done with.
        if let wl_buffer::Event::Release = event {
            buffer.destroy();
        }
    }
}

impl Dispatch<XdgWmBase, ()> for Client {
    fn event(
        _: &mut Self,
        wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Client {
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

delegate_noop!(Client: WlCompositor);
delegate_noop!(Client: WlShmPool);
delegate_noop!(Client: ignore WlSurface);
delegate_noop!(Client: ignore WlShm);
delegate_noop!(Client: ignore WlCallback);

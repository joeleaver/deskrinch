//! A throwaway Wayland client with one mapped `xdg_toplevel`: the windows the
//! compositor tests look at and act on.
//!
//! Each window is its own connection, so the compositor sees a separate
//! client, dispatched on a thread of its own so that it answers pings and
//! configures like a live application. A close request is recorded and
//! obeyed: the window unmaps, as an application would.

use std::fs::File;
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

const WIDTH: i32 = 320;
const HEIGHT: i32 = 240;

/// What the window's own thread reports.
#[derive(Debug, Default)]
struct Status {
    configured: bool,
    close_requested: bool,
}

#[derive(Debug)]
struct Client {
    status: Arc<Mutex<Status>>,
    surface: WlSurface,
    xdg_surface: XdgSurface,
    buffer: Option<WlBuffer>,
}

/// A mapped toplevel in a client of its own.
#[derive(Debug)]
pub(crate) struct TestWindow {
    connection: Connection,
    queue: QueueHandle<Client>,
    toplevel: XdgToplevel,
    surface: WlSurface,
    status: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TestWindow {
    /// Opens a window and returns once the compositor has mapped it.
    pub(crate) fn open(title: &str, app_id: &str) -> Self {
        let connection = Connection::connect_to_env().expect("connect the test window");
        let (globals, mut queue) =
            registry_queue_init::<Client>(&connection).expect("list globals");
        let qh = queue.handle();
        let compositor: WlCompositor = globals.bind(&qh, 1..=4, ()).expect("wl_compositor");
        let shm: WlShm = globals.bind(&qh, 1..=1, ()).expect("wl_shm");
        let wm_base: XdgWmBase = globals.bind(&qh, 1..=1, ()).expect("xdg_wm_base");

        let surface = compositor.create_surface(&qh, ());
        let xdg_surface = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg_surface.get_toplevel(&qh, ());
        toplevel.set_title(title.to_owned());
        toplevel.set_app_id(app_id.to_owned());
        surface.commit();

        let status = Arc::new(Mutex::new(Status::default()));
        let mut client = Client {
            status: Arc::clone(&status),
            surface: surface.clone(),
            xdg_surface,
            buffer: None,
        };
        while !status.lock().unwrap().configured {
            queue
                .blocking_dispatch(&mut client)
                .expect("first configure");
        }
        let buffer = shm_buffer(&shm, &qh);
        surface.attach(Some(&buffer), 0, 0);
        surface.damage(0, 0, WIDTH, HEIGHT);
        surface.commit();
        client.buffer = Some(buffer);
        queue.roundtrip(&mut client).expect("map the window");

        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let stop = Arc::clone(&stop);
            move || run(queue, client, &stop)
        });
        Self {
            connection,
            queue: qh,
            toplevel,
            surface,
            status,
            stop,
            thread: Some(thread),
        }
    }

    /// Changes the title.
    pub(crate) fn set_title(&self, title: &str) {
        self.toplevel.set_title(title.to_owned());
        self.surface.commit();
        self.connection.flush().expect("send the title");
    }

    /// Whether the compositor has asked this window to close.
    pub(crate) fn close_requested(&self) -> bool {
        self.status.lock().unwrap().close_requested
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.toplevel.destroy();
        self.surface.destroy();
        self.connection.display().sync(&self.queue, ());
        // The compositor may be gone already when a test fails.
        drop(self.connection.flush());
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

fn run(mut queue: EventQueue<Client>, mut client: Client, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        if queue.blocking_dispatch(&mut client).is_err() {
            return;
        }
    }
}

/// A buffer of transparent pixels, in a file of its own.
fn shm_buffer(shm: &WlShm, qh: &QueueHandle<Client>) -> WlBuffer {
    let stride = WIDTH * 4;
    let size = stride * HEIGHT;
    let file: File = tempfile::tempfile().expect("buffer file");
    file.set_len(u64::try_from(size).unwrap())
        .expect("size the buffer");
    let pool = shm.create_pool(file.as_fd(), size, qh, ());
    let buffer = pool.create_buffer(0, WIDTH, HEIGHT, stride, wl_shm::Format::Argb8888, qh, ());
    pool.destroy();
    buffer
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

impl Dispatch<XdgSurface, ()> for Client {
    fn event(
        client: &mut Self,
        _: &XdgSurface,
        event: xdg_surface::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            client.xdg_surface.ack_configure(serial);
            if client.buffer.is_some() {
                client.surface.commit();
            }
            client.status.lock().unwrap().configured = true;
        }
    }
}

impl Dispatch<XdgToplevel, ()> for Client {
    fn event(
        client: &mut Self,
        _: &XdgToplevel,
        event: xdg_toplevel::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Close = event {
            client.status.lock().unwrap().close_requested = true;
            // Obey, as an application would: unmap by dropping the buffer.
            client.surface.attach(None, 0, 0);
            client.surface.commit();
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
delegate_noop!(Client: ignore WlBuffer);
delegate_noop!(Client: ignore WlCallback);

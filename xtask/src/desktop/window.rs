//! Whether the desktop's window is still open: whether its compositor still
//! has an output.
//!
//! Each output of a nested compositor is a window. Closing it ends KWin, but
//! sway and labwc destroy the output and run on with none, showing nothing;
//! the desktop then ends too. A connection of xtask's own to the nested
//! compositor follows its `wl_output` globals.

use std::collections::HashSet;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use deskrinch_test_wait::Deadline;
use deskrinch_test_wayland::{WaitError, dispatch_until, roundtrip};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};

use super::DesktopError;

/// The outputs a compositor has, by global name.
#[derive(Debug, Default)]
struct Outputs(HashSet<u32>);

impl Dispatch<WlRegistry, ()> for Outputs {
    fn event(
        outputs: &mut Self,
        _: &WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name, interface, ..
            } if interface == "wl_output" => {
                outputs.0.insert(name);
            }
            wl_registry::Event::GlobalRemove { name } => {
                outputs.0.remove(&name);
            }
            _ => {}
        }
    }
}

/// The nested compositor's outputs, followed.
#[derive(Debug)]
pub(super) struct Window {
    queue: EventQueue<Outputs>,
    outputs: Outputs,
    _registry: WlRegistry,
}

impl Window {
    /// Follows the outputs of the compositor at `socket`.
    pub(super) fn follow(socket: &Path) -> Result<Self, DesktopError> {
        let watch = |source: Box<dyn std::error::Error + Send + Sync>| DesktopError::Watch {
            socket: socket.to_owned(),
            source,
        };
        let stream = UnixStream::connect(socket).map_err(|error| watch(error.into()))?;
        let connection = Connection::from_socket(stream).map_err(|error| watch(error.into()))?;
        let mut queue = connection.new_event_queue();
        let registry = connection.display().get_registry(&queue.handle(), ());
        let mut outputs = Outputs::default();
        roundtrip(
            &connection,
            &mut queue,
            &mut outputs,
            // A compositor that is gone fails the roundtrip at once.
            Deadline::patience(),
        )
        .map_err(|error| watch(error.into()))?;
        Ok(Self {
            queue,
            outputs,
            _registry: registry,
        })
    }

    /// Waits up to `wait` for the window to close; what was seen.
    pub(super) fn watch(&mut self, wait: Duration) -> Seen {
        let waited = dispatch_until(
            &mut self.queue,
            &mut self.outputs,
            Deadline::after(wait),
            |outputs| outputs.0.is_empty(),
        );
        match waited {
            Ok(()) => Seen::NoOutputs,
            Err(WaitError::TimedOut(_)) => Seen::Open,
            // The connection failed: the compositor is gone.
            Err(_) => Seen::Gone,
        }
    }
}

/// What following the window saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Seen {
    /// It is open.
    Open,
    /// The compositor runs on with no output: every window was closed
    /// (sway and labwc).
    NoOutputs,
    /// The compositor is gone: its window was closed (KWin), or it ended
    /// for another reason.
    Gone,
}

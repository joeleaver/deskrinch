//! The Wayland side: the connection, the outputs, shared-memory buffers,
//! and the two capture protocols ([`ext`] and [`wlr`]).

mod ext;
mod wlr;

use std::time::Duration;

use deskrinch_test_wayland::{Deadline, Globals, ShmBuffer, WaitError, dispatch_until, roundtrip};
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop};

use crate::Transform;
use crate::buffer::Layout;
use crate::error::{CaptureError, ConnectError};
use crate::image::Image;
use crate::screen::{CONNECT_PATIENCE, Method};

/// The `wl_output` version that names outputs.
const OUTPUT_NAMES: u32 = 4;

/// How the connection captures, when the compositor offers a protocol.
#[derive(Debug)]
pub(crate) enum Protocol {
    Ext(ext::Ext),
    Wlr(wlr::Wlr),
}

/// One output, as `wl_output` describes it.
#[derive(Debug)]
pub(crate) struct Output {
    pub(crate) proxy: WlOutput,
    pub(crate) name: Option<String>,
    pub(crate) transform: Transform,
}

/// What the queue's events build up.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) outputs: Vec<Output>,
    /// The capture in progress.
    pub(crate) frame: Frame,
}

/// What the compositor has said about the capture in progress.
#[derive(Debug, Default)]
pub(crate) struct Frame {
    /// The buffer size and the `wl_shm` formats it may have.
    pub(crate) size: Option<(u32, u32)>,
    pub(crate) formats: Vec<u32>,
    /// wlr-screencopy: the strides offered with each format.
    pub(crate) strides: Vec<(u32, u32)>,
    /// The constraints are complete.
    pub(crate) described: bool,
    pub(crate) transform: Option<Transform>,
    pub(crate) y_inverted: bool,
    /// `Ok` once copied, `Err` with the reason once failed.
    pub(crate) outcome: Option<Result<(), String>>,
}

/// The connection, and what it has learnt.
#[derive(Debug)]
pub(crate) struct Wayland {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    shm: WlShm,
    protocol: Option<Protocol>,
}

impl Wayland {
    /// Connects to `WAYLAND_DISPLAY`, binds every output and the first
    /// capture protocol the compositor offers that `allowed` takes, if any.
    pub(crate) fn connect(allowed: impl Fn(Method) -> bool) -> Result<Self, ConnectError> {
        let connection = Connection::connect_to_env().map_err(ConnectError::Wayland)?;
        let mut queue = connection.new_event_queue();
        let mut state = State::default();
        let globals = Globals::list(
            &connection,
            &mut queue,
            &mut state,
            Deadline::after(CONNECT_PATIENCE),
        )
        .map_err(ConnectError::Wait)?;
        let qh = queue.handle();
        let shm: WlShm = globals
            .bind(&qh, 1..=1, ())
            .map_err(|source| ConnectError::Bind {
                interface: "wl_shm",
                source,
            })?;
        let ext = if allowed(Method::ExtImageCopyCapture) {
            ext::Ext::bind(&globals, &qh)
        } else {
            None
        };
        let protocol = match ext {
            Some(ext) => Some(Protocol::Ext(ext)),
            None if allowed(Method::WlrScreencopy) => {
                wlr::Wlr::bind(&globals, &qh).map(Protocol::Wlr)
            }
            None => None,
        };
        state.outputs = bind_outputs(&globals, &qh);
        roundtrip(
            &connection,
            &mut queue,
            &mut state,
            Deadline::after(CONNECT_PATIENCE),
        )
        .map_err(ConnectError::Wait)?;
        Ok(Self {
            connection,
            queue,
            state,
            shm,
            protocol,
        })
    }

    /// The capture protocol the compositor offers, if any.
    pub(crate) fn protocol(&self) -> Option<&Protocol> {
        self.protocol.as_ref()
    }

    /// The outputs' names, in the order the compositor announced them.
    pub(crate) fn output_names(&self) -> Vec<String> {
        self.state
            .outputs
            .iter()
            .filter_map(|output| output.name.clone())
            .collect()
    }

    /// Captures the output named `name` with the compositor's protocol,
    /// within `patience`.
    pub(crate) fn capture(
        &mut self,
        name: &str,
        patience: Duration,
    ) -> Result<Image, CaptureError> {
        let deadline = Deadline::after(patience);
        // Catch up on the outputs' state (a transform just changed, say).
        roundtrip(&self.connection, &mut self.queue, &mut self.state, deadline)
            .map_err(|error| waited(error, name))?;
        let index = self.output_index(name)?;
        self.state.frame = Frame::default();
        let mut cx = Capturing {
            qh: self.queue.handle(),
            queue: &mut self.queue,
            state: &mut self.state,
            shm: &self.shm,
            deadline,
            output: name,
        };
        match &self.protocol {
            Some(Protocol::Ext(ext)) => ext.capture(&mut cx, index),
            Some(Protocol::Wlr(wlr)) => wlr.capture(&mut cx, index),
            None => Err(CaptureError::Failed(
                "the compositor offers no Wayland capture protocol".into(),
            )),
        }
    }

    /// The index in [`State::outputs`] of the output named `name`.
    pub(crate) fn output_index(&self, name: &str) -> Result<usize, CaptureError> {
        self.state
            .outputs
            .iter()
            .position(|output| output.name.as_deref() == Some(name))
            .ok_or_else(|| CaptureError::UnknownOutput {
                name: name.to_owned(),
                known: self.output_names(),
            })
    }
}

/// Binds every `wl_output` the compositor announced.
fn bind_outputs(globals: &Globals, qh: &QueueHandle<State>) -> Vec<Output> {
    globals
        .all()
        .into_iter()
        .filter(|global| global.interface == WlOutput::interface().name)
        .map(|global| Output {
            proxy: globals.bind_global(&global, global.version.min(OUTPUT_NAMES), qh, ()),
            name: None,
            transform: Transform::Normal,
        })
        .collect()
}

/// One capture under way: the connection's queue and state, and the
/// deadline the capture of `output` must meet.
#[derive(Debug)]
pub(crate) struct Capturing<'a> {
    pub(crate) queue: &'a mut EventQueue<State>,
    pub(crate) state: &'a mut State,
    pub(crate) shm: &'a WlShm,
    pub(crate) qh: QueueHandle<State>,
    pub(crate) deadline: Deadline,
    pub(crate) output: &'a str,
}

impl Capturing<'_> {
    /// Dispatches until `done` says the frame has got far enough, or the
    /// deadline passes.
    pub(crate) fn until(&mut self, done: impl Fn(&Frame) -> bool) -> Result<(), CaptureError> {
        dispatch_until(self.queue, self.state, self.deadline, |state| {
            done(&state.frame)
        })
        .map_err(|error| waited(error, self.output))
    }

    /// A buffer for the compositor to copy the output into.
    pub(crate) fn buffer(
        &self,
        size: (u32, u32),
        stride: u32,
        format: u32,
    ) -> Result<ShmBuffer, CaptureError> {
        let format = wl_shm::Format::try_from(format)
            .map_err(|()| CaptureError::UnsupportedFormat(format!("{format:#010x}")))?;
        ShmBuffer::new(self.shm, &self.qh, size, stride, format, None, ())
            .map_err(CaptureError::Buffer)
    }
}

/// The capture error for a wait on `output` that ended in `error`.
pub(crate) fn waited(error: WaitError, output: &str) -> CaptureError {
    match error {
        WaitError::TimedOut(patience) => CaptureError::TimedOut {
            output: output.to_owned(),
            patience,
        },
        other => CaptureError::Connection(other),
    }
}

/// The `wl_shm` formats this crate reads, in the order it prefers them, with
/// their byte layouts.
const SHM_FORMATS: [(wl_shm::Format, Layout); 4] = [
    (wl_shm::Format::Xrgb8888, Layout::Bgrx),
    (wl_shm::Format::Argb8888, Layout::Bgra),
    (wl_shm::Format::Xbgr8888, Layout::Rgbx),
    (wl_shm::Format::Abgr8888, Layout::Rgba),
];

/// The format to ask for among `offered`, and its layout.
pub(crate) fn choose_format(offered: &[u32]) -> Result<(u32, Layout), CaptureError> {
    SHM_FORMATS
        .iter()
        .map(|&(format, layout)| (u32::from(format), layout))
        .find(|(format, _)| offered.contains(format))
        .ok_or_else(|| {
            CaptureError::UnsupportedFormat(
                offered
                    .iter()
                    .map(|format| format!("{format:#010x}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        })
}

/// A `wl_shm` value as a number, known to the bindings or not.
pub(crate) fn shm_format(format: WEnum<wl_shm::Format>) -> u32 {
    match format {
        WEnum::Value(format) => format.into(),
        WEnum::Unknown(value) => value,
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &WlOutput,
        event: wl_output::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state
            .outputs
            .iter_mut()
            .find(|output| output.proxy == *proxy)
        else {
            return;
        };
        match event {
            wl_output::Event::Name { name } => output.name = Some(name),
            wl_output::Event::Geometry {
                transform: WEnum::Value(transform),
                ..
            } => {
                output.transform =
                    Transform::from_wayland(transform.into()).unwrap_or(Transform::Normal);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlBuffer, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlBuffer,
        _: wl_buffer::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // A capture buffer is the client's from the start; a release means
        // nothing for it.
    }
}

delegate_noop!(State: ignore WlShm);
delegate_noop!(State: WlShmPool);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wait_that_ran_out_names_the_output() {
        let error = waited(WaitError::TimedOut(Duration::from_secs(60)), "HEADLESS-1");
        assert!(
            matches!(&error, CaptureError::TimedOut { output, patience }
                if output == "HEADLESS-1" && *patience == Duration::from_secs(60)),
            "{error:?}"
        );
        assert!(matches!(
            waited(WaitError::Closed, "x"),
            CaptureError::Connection(_)
        ));
    }

    #[test]
    fn prefers_formats_without_alpha() {
        let argb = u32::from(wl_shm::Format::Argb8888);
        let xrgb = u32::from(wl_shm::Format::Xrgb8888);
        let abgr = u32::from(wl_shm::Format::Abgr8888);
        assert_eq!(choose_format(&[argb, xrgb]).unwrap(), (xrgb, Layout::Bgrx));
        assert_eq!(choose_format(&[abgr]).unwrap(), (abgr, Layout::Rgba));
        let error = choose_format(&[0x2020_3852]).unwrap_err();
        assert!(
            matches!(&error, CaptureError::UnsupportedFormat(listed) if listed == "0x20203852"),
            "{error:?}"
        );
    }
}

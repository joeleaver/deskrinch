//! `ext-image-copy-capture-v1`, with outputs as its sources
//! (`ext-image-capture-source-v1`): the standard protocol, and the one this
//! crate prefers.

use wayland_client::globals::GlobalList;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::ext::image_capture_source::v1::client::ext_image_capture_source_v1::ExtImageCaptureSourceV1;
use wayland_protocols::ext::image_capture_source::v1::client::ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1;
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_frame_v1::{
    self, ExtImageCopyCaptureFrameV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_manager_v1::{
    ExtImageCopyCaptureManagerV1, Options,
};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_session_v1::{
    self, ExtImageCopyCaptureSessionV1,
};

use super::{ShmBuffer, State, choose_format, dispatch_until, shm_format};
use crate::buffer::Buffer;
use crate::error::CaptureError;
use crate::image::Image;
use crate::transform::Transform;

/// The two managers the protocol needs.
#[derive(Debug)]
pub(crate) struct Ext {
    manager: ExtImageCopyCaptureManagerV1,
    sources: ExtOutputImageCaptureSourceManagerV1,
}

impl Ext {
    /// Binds the managers, when the compositor offers both.
    pub(super) fn bind(globals: &GlobalList, qh: &QueueHandle<State>) -> Option<Self> {
        let manager = globals.bind::<ExtImageCopyCaptureManagerV1, _, _>(qh, 1..=1, ());
        let sources = globals.bind::<ExtOutputImageCaptureSourceManagerV1, _, _>(qh, 1..=1, ());
        match (manager, sources) {
            (Ok(manager), Ok(sources)) => Some(Self { manager, sources }),
            (Ok(manager), Err(_)) => {
                manager.destroy();
                None
            }
            (Err(_), Ok(sources)) => {
                sources.destroy();
                None
            }
            (Err(_), Err(_)) => None,
        }
    }

    /// Captures the output at `index` once.
    pub(super) fn capture(
        &self,
        queue: &mut EventQueue<State>,
        state: &mut State,
        shm: &WlShm,
        qh: &QueueHandle<State>,
        index: usize,
    ) -> Result<Image, CaptureError> {
        let source = self
            .sources
            .create_source(&state.outputs[index].proxy, qh, ());
        let session = self
            .manager
            .create_session(&source, Options::empty(), qh, ());
        let result = capture_in(queue, state, shm, qh, &session);
        session.destroy();
        source.destroy();
        result
    }
}

fn capture_in(
    queue: &mut EventQueue<State>,
    state: &mut State,
    shm: &WlShm,
    qh: &QueueHandle<State>,
    session: &ExtImageCopyCaptureSessionV1,
) -> Result<Image, CaptureError> {
    dispatch_until(queue, state, |frame| {
        frame.described || frame.outcome.is_some()
    })?;
    if let Some(Err(reason)) = state.frame.outcome.take() {
        return Err(CaptureError::Failed(reason));
    }
    let size = state
        .frame
        .size
        .ok_or_else(|| CaptureError::Failed("the session gave no buffer size".into()))?;
    let (format, layout) = choose_format(&state.frame.formats)?;
    let stride = size.0 * 4;
    let buffer = ShmBuffer::new(shm, qh, size, stride, format)?;
    let frame = session.create_frame(qh, ());
    frame.attach_buffer(&buffer.proxy);
    let (width, height) = (
        i32::try_from(size.0).unwrap_or(i32::MAX),
        i32::try_from(size.1).unwrap_or(i32::MAX),
    );
    frame.damage_buffer(0, 0, width, height);
    frame.capture();
    let copied = dispatch_until(queue, state, |frame| frame.outcome.is_some());
    frame.destroy();
    copied?;
    if let Some(Err(reason)) = state.frame.outcome.take() {
        return Err(CaptureError::Failed(reason));
    }
    let bytes = buffer.read()?;
    Buffer {
        bytes: &bytes,
        width: size.0,
        height: size.1,
        stride,
        layout,
        y_inverted: false,
        transform: state.frame.transform.unwrap_or(Transform::Normal),
    }
    .to_image()
}

impl Dispatch<ExtImageCopyCaptureSessionV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_session_v1::Event;
        let frame = &mut state.frame;
        match event {
            Event::BufferSize { width, height } => {
                // Each round of constraints starts afresh.
                frame.size = Some((width, height));
                frame.formats.clear();
                frame.described = false;
            }
            Event::ShmFormat { format } => frame.formats.push(shm_format(format)),
            Event::Done => frame.described = true,
            Event::Stopped => {
                frame.outcome = Some(Err("the compositor stopped the capture session".into()));
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_frame_v1::{Event, FailureReason};
        let frame = &mut state.frame;
        match event {
            Event::Transform { transform } => {
                frame.transform = match transform {
                    WEnum::Value(transform) => Transform::from_wayland(transform.into()),
                    WEnum::Unknown(value) => Transform::from_wayland(value),
                };
            }
            Event::Ready => frame.outcome = Some(Ok(())),
            Event::Failed { reason } => {
                let reason = match reason {
                    WEnum::Value(FailureReason::BufferConstraints) => {
                        "the buffer did not meet the constraints".to_owned()
                    }
                    WEnum::Value(FailureReason::Stopped) => "the session stopped".to_owned(),
                    WEnum::Value(other) => format!("{other:?}"),
                    WEnum::Unknown(value) => format!("reason {value}"),
                };
                frame.outcome = Some(Err(reason));
            }
            _ => {}
        }
    }
}

delegate_noop!(State: ExtImageCopyCaptureManagerV1);
delegate_noop!(State: ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(State: ExtImageCaptureSourceV1);

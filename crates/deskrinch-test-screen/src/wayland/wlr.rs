//! `wlr-screencopy-unstable-v1`: for a wlroots compositor that does not
//! offer `ext-image-copy-capture-v1` yet.

use deskrinch_test_wayland::Globals;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::{
    self, ZwlrScreencopyFrameV1,
};
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

use super::{Capturing, State, choose_format, shm_format};
use crate::buffer::Buffer;
use crate::error::CaptureError;
use crate::image::Image;

/// The first version that says when every buffer type has been offered.
const BUFFER_DONE: u32 = 3;

/// The protocol's manager.
#[derive(Debug)]
pub(crate) struct Wlr {
    manager: ZwlrScreencopyManagerV1,
}

impl Wlr {
    /// Binds the manager, when the compositor offers it.
    pub(super) fn bind(globals: &Globals, qh: &QueueHandle<State>) -> Option<Self> {
        globals
            .bind::<ZwlrScreencopyManagerV1, _, _>(qh, 1..=BUFFER_DONE, ())
            .ok()
            .map(|manager| Self { manager })
    }

    /// Captures the output at `index` once.
    pub(super) fn capture(
        &self,
        cx: &mut Capturing<'_>,
        index: usize,
    ) -> Result<Image, CaptureError> {
        let frame = self
            .manager
            .capture_output(0, &cx.state.outputs[index].proxy, &cx.qh, ());
        let result = capture_in(cx, &frame, index);
        frame.destroy();
        result
    }
}

fn capture_in(
    cx: &mut Capturing<'_>,
    frame: &ZwlrScreencopyFrameV1,
    index: usize,
) -> Result<Image, CaptureError> {
    // Before version 3 the one `buffer` event is the whole offer.
    let lists_all = frame.version() >= BUFFER_DONE;
    cx.until(|frame| {
        frame.outcome.is_some() || frame.described || (!lists_all && !frame.formats.is_empty())
    })?;
    let state = &mut *cx.state;
    if let Some(Err(reason)) = state.frame.outcome.take() {
        return Err(CaptureError::Failed(reason));
    }
    let size = state
        .frame
        .size
        .ok_or_else(|| CaptureError::Failed("the frame offered no shared-memory buffer".into()))?;
    let (format, layout) = choose_format(&state.frame.formats)?;
    let stride = state
        .frame
        .strides
        .iter()
        .find_map(|&(offered, stride)| (offered == format).then_some(stride))
        .unwrap_or(size.0 * 4);
    let buffer = cx.buffer(size, stride, format)?;
    frame.copy(buffer.buffer());
    cx.until(|frame| frame.outcome.is_some())?;
    let state = &mut *cx.state;
    if let Some(Err(reason)) = state.frame.outcome.take() {
        return Err(CaptureError::Failed(reason));
    }
    let bytes = buffer.read().map_err(CaptureError::Buffer)?;
    Buffer {
        bytes: &bytes,
        width: size.0,
        height: size.1,
        stride,
        layout,
        y_inverted: state.frame.y_inverted,
        // The protocol copies the output's buffer as it is, transformed.
        transform: state.outputs[index].transform,
    }
    .to_image()
}

impl Dispatch<ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_screencopy_frame_v1::{Event, Flags};
        let frame = &mut state.frame;
        match event {
            Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let format = shm_format(format);
                frame.size = Some((width, height));
                frame.formats.push(format);
                frame.strides.push((format, stride));
            }
            Event::BufferDone => frame.described = true,
            Event::Flags { flags } => {
                frame.y_inverted = match flags {
                    WEnum::Value(flags) => flags.contains(Flags::YInvert),
                    WEnum::Unknown(bits) => bits & 1 == 1,
                };
            }
            Event::Ready { .. } => frame.outcome = Some(Ok(())),
            Event::Failed => {
                frame.outcome = Some(Err("the compositor could not copy the output".into()));
            }
            _ => {}
        }
    }
}

delegate_noop!(State: ZwlrScreencopyManagerV1);

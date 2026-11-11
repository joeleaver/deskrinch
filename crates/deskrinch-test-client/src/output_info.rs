//! What a [`TestClient`](crate::TestClient) knows about each output: its
//! whole scale and current mode from `wl_output`, and its logical size from
//! `xdg-output`, which together tell a fractional scale from a whole one.

use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::{self, ZxdgOutputV1};

use crate::client::Client;
use crate::lock;

/// One output, as the compositor describes it to a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputInfo {
    /// The scale `wl_output` reports: a whole number, rounded up from a
    /// fractional scale.
    pub scale: i32,
    /// The current mode's size in physical pixels, once reported.
    pub mode: Option<(i32, i32)>,
    /// The output's size in the logical layout (`xdg-output`), once
    /// reported: the mode divided by the scale, turned with the transform.
    pub logical_size: Option<(i32, i32)>,
}

impl Default for OutputInfo {
    fn default() -> Self {
        Self {
            scale: 1,
            mode: None,
            logical_size: None,
        }
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
        match event {
            wl_output::Event::Name { name } => {
                lock(&client.outputs).insert(name, output.clone());
            }
            wl_output::Event::Scale { factor } => {
                lock(&client.infos).entry(output.id()).or_default().scale = factor;
            }
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                ..
            } if flags.contains(wl_output::Mode::Current) => {
                lock(&client.infos).entry(output.id()).or_default().mode = Some((width, height));
            }
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputV1, ObjectId> for Client {
    fn event(
        client: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        output: &ObjectId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_output_v1::Event::LogicalSize { width, height } = event {
            lock(&client.infos)
                .entry(output.clone())
                .or_default()
                .logical_size = Some((width, height));
        }
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ZxdgOutputManagerV1,
        _: <ZxdgOutputManagerV1 as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

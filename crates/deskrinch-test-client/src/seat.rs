//! The seat: the client binds it when the compositor offers one, and
//! follows its capabilities, taking its keyboard ([`crate::keyboard`]) and
//! its pointer ([`crate::pointer`]) whenever it has them, and letting them go
//! when it no longer does. A seat says what it has again whenever that
//! changes, so a window opened before a device existed still hears it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_seat::{self, Capability, WlSeat};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};

use crate::client::Client;
use crate::window::Shared;

/// The client's windows by their surfaces, for the seat's events.
pub(crate) type Windows = Arc<Mutex<HashMap<ObjectId, Arc<Shared>>>>;

impl Dispatch<WlSeat, ()> for Client {
    fn event(
        client: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        else {
            return;
        };
        let keyboard = capabilities.contains(Capability::Keyboard);
        client.keyboard.follow(seat, keyboard, qh);
        let pointer = capabilities.contains(Capability::Pointer);
        client.pointer.follow(seat, pointer, qh);
    }
}

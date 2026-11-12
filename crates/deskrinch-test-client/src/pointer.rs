//! The pointer, as a test window hears it: where it entered, moved and
//! left, and the buttons pressed and released over it, in the window's
//! surface coordinates (logical pixels from its top-left corner), in the
//! order the compositor sent them.
//!
//! The client takes the seat's pointer whenever the seat has one
//! ([`crate::seat`]). Axis (wheel) events, frames and the pointer's cursor
//! are not kept.

use std::sync::Arc;

use wayland_client::protocol::wl_pointer::{self, ButtonState, WlPointer};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

use crate::client::Client;
use crate::window::{Shared, TestWindow};
use crate::{Error, lock};

/// One thing a window heard of the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    /// The pointer came over the window, at this point.
    Entered {
        /// Logical pixels from the surface's left edge.
        x: f64,
        /// Logical pixels from the surface's top edge.
        y: f64,
    },
    /// The pointer moved over the window, to this point.
    Moved {
        /// Logical pixels from the surface's left edge.
        x: f64,
        /// Logical pixels from the surface's top edge.
        y: f64,
    },
    /// A button went down, by its evdev code (`BTN_LEFT` is 272).
    Pressed {
        /// The button's evdev code.
        button: u32,
    },
    /// A button came up, by its evdev code.
    Released {
        /// The button's evdev code.
        button: u32,
    },
    /// The pointer left the window.
    Left,
}

impl PointerEvent {
    /// The point an `Entered` or `Moved` event names.
    #[must_use]
    pub fn point(&self) -> Option<(f64, f64)> {
        match *self {
            Self::Entered { x, y } | Self::Moved { x, y } => Some((x, y)),
            _ => None,
        }
    }
}

/// The pointer's state on the client's thread.
#[derive(Debug, Default)]
pub(crate) struct Pointer {
    pointer: Option<WlPointer>,
    /// The window the pointer is over.
    focus: Option<Arc<Shared>>,
}

impl Pointer {
    /// Takes the seat's pointer when it `has` one, and lets it go when it no
    /// longer does, as a [`PointerEvent::Left`] for the window it was over.
    /// KWin takes the pointer away whenever its last fake-input device goes,
    /// and gives it back a moment after the next one comes, not before it
    /// is used: a test waits for a window to hear the pointer at a point
    /// before it presses a button there.
    pub(crate) fn follow(&mut self, seat: &WlSeat, has: bool, qh: &QueueHandle<Client>) {
        match (&self.pointer, has) {
            (None, true) => self.pointer = Some(seat.get_pointer(qh, ())),
            (Some(pointer), false) => {
                pointer.release();
                self.pointer = None;
                // The pointer is no longer over it: the window hears it leave,
                // and waits for the next pointer's enter.
                if let Some(window) = self.focus.take() {
                    hear(&window, PointerEvent::Left);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlPointer, ()> for Client {
    fn event(
        client: &mut Self,
        _: &WlPointer,
        event: wl_pointer::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let heard = match event {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } => {
                client.pointer.focus = lock(&client.windows).get(&surface.id()).cloned();
                PointerEvent::Entered {
                    x: surface_x,
                    y: surface_y,
                }
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => PointerEvent::Moved {
                x: surface_x,
                y: surface_y,
            },
            wl_pointer::Event::Button {
                button,
                state: WEnum::Value(state),
                ..
            } => match state {
                ButtonState::Pressed => PointerEvent::Pressed { button },
                _ => PointerEvent::Released { button },
            },
            wl_pointer::Event::Leave { .. } => {
                if let Some(window) = client.pointer.focus.take() {
                    hear(&window, PointerEvent::Left);
                }
                return;
            }
            _ => return,
        };
        if let Some(window) = &client.pointer.focus {
            hear(window, heard);
        }
    }
}

fn hear(window: &Shared, event: PointerEvent) {
    lock(&window.status).input.pointer.push(event);
    window.changed.notify_all();
}

impl TestWindow {
    /// What it heard of the pointer, in order.
    #[must_use]
    pub fn pointer_events(&self) -> Vec<PointerEvent> {
        lock(&self.shared.status).input.pointer.clone()
    }

    /// Where the pointer is over it, as its last `Entered` or `Moved` said;
    /// `None` when the pointer is not over it.
    #[must_use]
    pub fn pointer_position(&self) -> Option<(f64, f64)> {
        position(&lock(&self.shared.status).input.pointer)
    }

    /// Waits until what it heard of the pointer satisfies `done`; `what`
    /// names it for the error.
    ///
    /// # Errors
    ///
    /// When it does not within [`PATIENCE`](crate::PATIENCE).
    pub fn wait_pointer(
        &self,
        what: &str,
        done: impl Fn(&[PointerEvent]) -> bool,
    ) -> Result<(), Error> {
        self.wait_input(what, |status| done(&status.input.pointer))
    }

    /// Waits until the pointer is over it at `(x, y)`, in its surface
    /// coordinates.
    ///
    /// # Errors
    ///
    /// When it is not within [`PATIENCE`](crate::PATIENCE).
    pub fn wait_pointer_at(&self, x: f64, y: f64) -> Result<(), Error> {
        self.wait_pointer(&format!("the pointer at ({x}, {y})"), |events| {
            position(events) == Some((x, y))
        })
    }

    /// A mark in what it has heard of the pointer: what comes after it was
    /// heard later ([`wait_pointer_at_since`](Self::wait_pointer_at_since)).
    #[must_use]
    pub fn pointer_mark(&self) -> PointerMark {
        PointerMark(lock(&self.shared.status).input.pointer.len())
    }

    /// Waits until it has heard, since `mark`, the pointer come to `(x, y)`
    /// and not leave. Unlike [`wait_pointer_at`](Self::wait_pointer_at), a
    /// position heard before the mark does not count: the pointer may have
    /// been left there by an earlier test, or by a device since gone.
    ///
    /// # Errors
    ///
    /// When it does not within [`PATIENCE`](crate::PATIENCE).
    pub fn wait_pointer_at_since(&self, mark: PointerMark, x: f64, y: f64) -> Result<(), Error> {
        self.wait_pointer(&format!("the pointer at ({x}, {y}) again"), |events| {
            at_since(events, mark, (x, y))
        })
    }
}

/// A place in a window's pointer events ([`TestWindow::pointer_mark`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerMark(usize);

/// Whether `events` since `mark` leave the pointer at `point`.
fn at_since(events: &[PointerEvent], mark: PointerMark, point: (f64, f64)) -> bool {
    events
        .get(mark.0..)
        .is_some_and(|since| position(since) == Some(point))
}

/// Where `events` leave the pointer: the last point named, unless it left
/// after it.
fn position(events: &[PointerEvent]) -> Option<(f64, f64)> {
    events
        .iter()
        .rev()
        .find_map(|event| match event {
            PointerEvent::Left => Some(None),
            other => other.point().map(Some),
        })
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_heard_before_the_mark_does_not_count() {
        use PointerEvent::{Entered, Left, Moved};
        let events = [Entered { x: 1.0, y: 2.0 }];
        assert!(
            !at_since(&events, PointerMark(1), (1.0, 2.0)),
            "nothing since"
        );
        let events = [Entered { x: 1.0, y: 2.0 }, Moved { x: 1.0, y: 2.0 }];
        assert!(at_since(&events, PointerMark(1), (1.0, 2.0)));
        let events = [Entered { x: 1.0, y: 2.0 }, Moved { x: 1.0, y: 2.0 }, Left];
        assert!(!at_since(&events, PointerMark(1), (1.0, 2.0)), "left since");
    }

    #[test]
    fn the_position_is_the_last_point_until_it_leaves() {
        use PointerEvent::{Entered, Left, Moved, Pressed};
        assert_eq!(position(&[]), None);
        let entered = [Entered { x: 1.0, y: 2.0 }, Pressed { button: 272 }];
        assert_eq!(position(&entered), Some((1.0, 2.0)));
        let moved = [Entered { x: 1.0, y: 2.0 }, Moved { x: 3.0, y: 4.0 }];
        assert_eq!(position(&moved), Some((3.0, 4.0)));
        let left = [Entered { x: 1.0, y: 2.0 }, Left];
        assert_eq!(position(&left), None);
        let back = [Entered { x: 1.0, y: 2.0 }, Left, Entered { x: 5.0, y: 6.0 }];
        assert_eq!(position(&back), Some((5.0, 6.0)));
    }
}

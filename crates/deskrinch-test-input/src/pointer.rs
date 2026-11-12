//! The pointer: moved to a point on a named output ([`crate::layout`]), and
//! its buttons pressed and released.
//!
//! Through wlroots' virtual pointer, every request is followed by a `frame`,
//! as a real pointer's events come in frames. KWin's fake input has no
//! frames of its own.

use wayland_client::protocol::wl_pointer::ButtonState;

use crate::backend::{Backend, PRESSED, RELEASED};
use crate::layout::{Layout, against_extent};
use crate::{Button, Error, Input};

impl Input {
    /// Moves the pointer to `(x, y)` on the output named `output`, in that
    /// output's logical pixels from its top-left corner.
    ///
    /// # Errors
    ///
    /// [`Error::NoPointer`] when the compositor cannot make one,
    /// [`Error::NoOutput`] and [`Error::OffOutput`] for a point not on an
    /// output, [`Error::Unplaced`] when an output's place is not known, and
    /// otherwise as [`press`](Self::press).
    pub fn move_pointer(&mut self, output: &str, x: f64, y: f64) -> Result<(), Error> {
        let layout = Layout::of(&self.outputs)?;
        let point = layout.global(output, x, y)?;
        let time = self.time();
        match &self.backend {
            Backend::Wlroots { pointer, .. } => {
                let pointer = pointer.as_ref().ok_or(Error::NoPointer)?;
                let (x, y, x_extent, y_extent) = against_extent(layout.bounds(), point);
                pointer.motion_absolute(time, x, y, x_extent, y_extent);
                pointer.frame();
            }
            Backend::Kwin(fake) => fake.pointer_motion_absolute(point.0, point.1),
        }
        self.settle()
    }

    /// Presses `button` where the pointer is, and holds it until
    /// [`release_button`](Self::release_button), or until this `Input` is
    /// dropped, which releases it.
    ///
    /// # Errors
    ///
    /// [`Error::NoPointer`] when the compositor cannot make one; otherwise
    /// as [`press`](Self::press).
    pub fn press_button(&mut self, button: Button) -> Result<(), Error> {
        self.button(button, true)?;
        if !self.held.contains(&button) {
            self.held.push(button);
        }
        self.settle()
    }

    /// Releases `button` where the pointer is.
    ///
    /// # Errors
    ///
    /// As [`press_button`](Self::press_button).
    pub fn release_button(&mut self, button: Button) -> Result<(), Error> {
        self.button(button, false)?;
        self.held.retain(|held| *held != button);
        self.settle()
    }

    /// Presses `button` where the pointer is and releases it there: a
    /// click.
    ///
    /// # Errors
    ///
    /// As [`press_button`](Self::press_button).
    pub fn click(&mut self, button: Button) -> Result<(), Error> {
        self.button(button, true)?;
        self.button(button, false)?;
        self.held.retain(|held| *held != button);
        self.settle()
    }

    fn button(&self, button: Button, pressed: bool) -> Result<(), Error> {
        match &self.backend {
            Backend::Wlroots { pointer, .. } => {
                let pointer = pointer.as_ref().ok_or(Error::NoPointer)?;
                let state = if pressed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                };
                pointer.button(self.time(), button.code(), state);
                pointer.frame();
            }
            Backend::Kwin(fake) => {
                fake.button(button.code(), if pressed { PRESSED } else { RELEASED });
            }
        }
        Ok(())
    }
}

/// An `Input` leaves nothing behind it.
///
/// - A button held is released: a test that fails between
///   [`press_button`](Input::press_button) and
///   [`release_button`](Input::release_button) would otherwise leave the
///   button down for every later test in the session (KWin's fake input
///   destroys nothing with the client).
/// - Its devices are destroyed, and the compositor has removed them when
///   the drop returns. Left to the connection's close, a compositor removes
///   them when it notices it, which can be after the next `Input`, of this
///   process or the next, has made its own; the seat then takes its pointer
///   away from every client and gives it back, and what is pressed in
///   between reaches no window.
///
/// Best effort: the connection may be what failed, so errors are ignored.
impl Drop for Input {
    fn drop(&mut self) {
        for button in std::mem::take(&mut self.held) {
            let _unreleased = self.button(button, false);
        }
        self.backend.destroy();
        let _unsettled = self.settle();
    }
}

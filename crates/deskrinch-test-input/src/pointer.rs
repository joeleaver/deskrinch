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

    /// Presses `button` where the pointer is, and holds it.
    ///
    /// # Errors
    ///
    /// [`Error::NoPointer`] when the compositor cannot make one; otherwise
    /// as [`press`](Self::press).
    pub fn press_button(&mut self, button: Button) -> Result<(), Error> {
        self.button(button, true)?;
        self.settle()
    }

    /// Releases `button` where the pointer is.
    ///
    /// # Errors
    ///
    /// As [`press_button`](Self::press_button).
    pub fn release_button(&mut self, button: Button) -> Result<(), Error> {
        self.button(button, false)?;
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

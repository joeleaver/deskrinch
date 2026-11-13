//! What deskrinch's Wayland test-support crates share: the colours and
//! transforms pictures are described in, `wl_shm` buffers in memory files of their
//! own, and waiting for a compositor with a deadline.
//!
//! - [`Rgba`]: a pixel, as `deskrinch-test-screen` reports captures and
//!   `deskrinch-test-client` paints windows.
//! - [`Transform`]: `wl_output.transform`, and where a transformed buffer
//!   holds a pixel of the upright picture.
//! - [`ShmBuffer`]: a `wl_buffer` in a memory file ([`memory_file`]), for a client to fill
//!   or a compositor to copy into.
//! - [`Globals`]: the compositor's globals, listed within a deadline.
//! - [`dispatch_until`] and [`roundtrip`]: dispatching a queue until a
//!   condition holds or a [`Deadline`] passes, so a compositor that never
//!   answers fails a test with [`WaitError::TimedOut`] instead of hanging
//!   it.

mod color;
mod globals;
mod memory;
mod shm;
mod transform;
mod wait;

pub use color::Rgba;
pub use globals::{BindError, Global, Globals};
pub use memory::memory_file;
pub use shm::{ShmBuffer, ShmError};
pub use transform::Transform;
pub use wait::{Deadline, WaitError, dispatch_until, roundtrip};

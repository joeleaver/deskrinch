//! A `wl_shm` buffer in a memory file of its own ([`memory_file`]).
//!
//! [`memory_file`]: crate::memory_file

use std::fs::File;
use std::io;
use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;

use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::{Dispatch, QueueHandle};

use crate::memory::memory_file;

/// Why a buffer could not be made or read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ShmError {
    /// Its size does not fit `wl_shm`'s 32-bit sizes.
    #[error("a {width}x{height} buffer with stride {stride} is too large for wl_shm")]
    TooLarge {
        /// The width asked for.
        width: u32,
        /// The height asked for.
        height: u32,
        /// The stride asked for.
        stride: u32,
    },
    /// The contents given are not the buffer's size.
    #[error("the contents are {given} bytes, not the buffer's {size}")]
    WrongContents {
        /// The bytes given.
        given: usize,
        /// The buffer's size.
        size: usize,
    },
    /// The file could not be made, sized, written or read.
    #[error("could not {what} the buffer's file")]
    Io {
        /// What was being done.
        what: &'static str,
        /// Why it failed.
        #[source]
        source: io::Error,
    },
}

/// A `wl_buffer` of `width` × `height` pixels, `stride` bytes per row, in
/// a file only it uses. Dropping it destroys the `wl_buffer`, unless
/// [`into_buffer`](Self::into_buffer) handed it on.
#[derive(Debug)]
pub struct ShmBuffer {
    file: File,
    buffer: WlBuffer,
    len: usize,
    owned: bool,
}

impl ShmBuffer {
    /// A buffer in `format`, zero-filled, or holding `contents` (exactly its
    /// `stride * height` bytes) when given. `data` is the `wl_buffer`'s user
    /// data.
    ///
    /// # Errors
    ///
    /// When the size does not fit `wl_shm`, the contents are not the
    /// buffer's size, or the file cannot be made or written.
    pub fn new<S, U>(
        shm: &WlShm,
        qh: &QueueHandle<S>,
        (width, height): (u32, u32),
        stride: u32,
        format: wl_shm::Format,
        contents: Option<&[u8]>,
        data: U,
    ) -> Result<Self, ShmError>
    where
        S: Dispatch<WlShmPool, ()> + Dispatch<WlBuffer, U> + 'static,
        U: Send + Sync + 'static,
    {
        let too_large = || ShmError::TooLarge {
            width,
            height,
            stride,
        };
        let len = u64::from(stride) * u64::from(height);
        let pool_size = i32::try_from(len).map_err(|_| too_large())?;
        let len = usize::try_from(len).map_err(|_| too_large())?;
        let signed = |value: u32| i32::try_from(value).map_err(|_| too_large());
        let (w, h, s) = (signed(width)?, signed(height)?, signed(stride)?);
        if let Some(contents) = contents
            && contents.len() != len
        {
            return Err(ShmError::WrongContents {
                given: contents.len(),
                size: len,
            });
        }
        let io_error = |what| move |source| ShmError::Io { what, source };
        let file = memory_file("deskrinch-wl-shm").map_err(io_error("create"))?;
        file.set_len(u64::try_from(len).map_err(|_| too_large())?)
            .map_err(io_error("size"))?;
        if let Some(contents) = contents {
            file.write_all_at(contents, 0).map_err(io_error("write"))?;
        }
        let pool = shm.create_pool(file.as_fd(), pool_size, qh, ());
        let buffer = pool.create_buffer(0, w, h, s, format, qh, data);
        pool.destroy();
        Ok(Self {
            file,
            buffer,
            len,
            owned: true,
        })
    }

    /// The `wl_buffer`.
    #[must_use]
    pub fn buffer(&self) -> &WlBuffer {
        &self.buffer
    }

    /// What the buffer holds now: what the compositor wrote into it.
    ///
    /// # Errors
    ///
    /// When the file cannot be read.
    pub fn read(&self) -> Result<Vec<u8>, ShmError> {
        let mut bytes = vec![0; self.len];
        self.file
            .read_exact_at(&mut bytes, 0)
            .map_err(|source| ShmError::Io {
                what: "read",
                source,
            })?;
        Ok(bytes)
    }

    /// Hands the `wl_buffer` on, to be destroyed by whoever holds it (when
    /// the compositor releases it, say); the file is closed, and the
    /// compositor keeps its own mapping.
    #[must_use]
    pub fn into_buffer(mut self) -> WlBuffer {
        self.owned = false;
        self.buffer.clone()
    }
}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        if self.owned {
            self.buffer.destroy();
        }
    }
}

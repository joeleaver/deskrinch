//! [`memory_file`]: a file in memory, to share with the compositor.

use std::fs::File;
use std::io;

use rustix::fs::{MemfdFlags, memfd_create};

/// A new, empty file in memory (`memfd_create`), closed on exec, for what a
/// test client shares with the compositor through a descriptor: a `wl_shm`
/// pool, a keymap, a screenshot KWin writes.
///
/// In memory, not on disk: it needs no room on the filesystem `/tmp` is on,
/// it never fills as a pipe does, and it is what real clients share a pool
/// in. `name` is shown in `/proc/<pid>/fd` only.
///
/// # Errors
///
/// When the kernel cannot make one (out of memory, too many files).
pub fn memory_file(name: &str) -> io::Result<File> {
    Ok(File::from(memfd_create(name, MemfdFlags::CLOEXEC)?))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, Write};

    use super::*;

    #[test]
    fn holds_what_is_written() {
        let mut file = memory_file("deskrinch-test").unwrap();
        file.write_all(b"pixels").unwrap();
        file.rewind().unwrap();
        let mut read = String::new();
        file.read_to_string(&mut read).unwrap();
        assert_eq!(read, "pixels");
        assert_eq!(file.metadata().unwrap().len(), 6);
    }
}

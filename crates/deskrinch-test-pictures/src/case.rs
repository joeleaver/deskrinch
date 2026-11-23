//! A named picture, and the noise pictures are made of: shared by the
//! library and the build script that makes the ordinary corpus.

/// A named picture of the corpus.
#[derive(Debug, Clone)]
pub struct Case {
    /// What it is.
    pub name: String,
    /// Its bytes.
    pub bytes: Vec<u8>,
}

/// The case `name` with `bytes`.
pub(crate) fn case(name: &str, bytes: Vec<u8>) -> Case {
    Case {
        name: name.to_owned(),
        bytes,
    }
}

/// Pixels that do not repeat, `n` bytes of them.
#[must_use]
pub fn noise(n: usize) -> Vec<u8> {
    let mut x: u32 = 0x9e37_79b9;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x.to_le_bytes()[0]
        })
        .collect()
}

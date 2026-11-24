//! Digests of files and bytes, in lower-case hex: the package cache's
//! checks ([`crate::ci_packages`]) and the compositor tests' bundle
//! ([`crate::compositor_tests`]) hold files to them.

use std::fmt::Write as _;
use std::fs::File;
use std::io::Read as _;
use std::path::Path;

use sha2::Digest;

use crate::error::Error;

/// The hex digest `D` of the file at `path`, read in blocks.
pub(crate) fn of_file<D: Digest>(path: &Path) -> Result<String, Error> {
    let io = |source| Error::Read {
        path: path.to_owned(),
        source,
    };
    let mut file = File::open(path).map_err(io)?;
    let mut hasher = D::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// `bytes` in lower-case hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, byte| {
        let _infallible = write!(hex, "{byte:02x}");
        hex
    })
}

#[cfg(test)]
mod tests {
    use sha2::Sha256;

    use super::*;

    #[test]
    fn a_file_s_digest_is_its_bytes_digest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        let bytes = vec![7_u8; 200_000];
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            of_file::<Sha256>(&path).unwrap(),
            hex(&Sha256::digest(&bytes))
        );
        assert_eq!(hex(&[0, 15, 255]), "000fff");
        assert!(of_file::<Sha256>(&dir.path().join("missing")).is_err());
    }
}

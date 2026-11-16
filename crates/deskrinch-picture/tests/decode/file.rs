//! A picture in a file a stranger named.

use deskrinch_picture::{DecodeError, Format, Formats, decode_file};
use deskrinch_test_pictures::fixtures;
use deskrinch_untrusted::file::FileError;

use crate::roomy;

#[test]
fn a_picture_file_is_decoded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a picture.png.jpg");
    std::fs::write(&path, fixtures::BASELINE_JPEG).unwrap();
    // Found from its signature, whatever its name says.
    let decoded = decode_file(&path, Formats::all(), &roomy()).unwrap();
    assert_eq!(decoded.format, Format::Jpeg);
}

#[test]
fn a_file_over_the_encoded_bound_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg");
    std::fs::write(&path, fixtures::BASELINE_JPEG).unwrap();
    let tight = deskrinch_picture::Limits {
        max_encoded_bytes: 100,
        ..roomy()
    };
    assert!(matches!(
        decode_file(&path, Formats::all(), &tight),
        Err(DecodeError::EncodedTooLarge { max: 100, .. })
    ));
}

#[test]
fn a_fifo_a_device_and_a_directory_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let pipe = dir.path().join("pipe");
    rustix::fs::mkfifoat(rustix::fs::CWD, &pipe, rustix::fs::Mode::RUSR).unwrap();
    assert!(matches!(
        decode_file(&pipe, Formats::all(), &roomy()),
        Err(DecodeError::File(FileError::Fifo))
    ));
    assert!(matches!(
        decode_file(std::path::Path::new("/dev/zero"), Formats::all(), &roomy()),
        Err(DecodeError::File(FileError::Device))
    ));
    assert!(matches!(
        decode_file(dir.path(), Formats::all(), &roomy()),
        Err(DecodeError::File(FileError::Directory))
    ));
}

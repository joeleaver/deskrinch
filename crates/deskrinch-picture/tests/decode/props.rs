//! Arbitrary and damaged bytes never panic, and never give a picture over
//! the limits. (That they never allocate past the working bound is
//! `tests/working_bound.rs`'s.)
//!
//! Each case's decode runs under the shared deadline
//! (`deskrinch_untrusted::fuzz::case_bytes`). A decode that hangs or runs
//! away ends the test binary with a sentence beginning "fuzz watchdog:",
//! naming the property and the file holding the bytes it was decoding,
//! unshrunk; an ordinary failure (a panic, a limit passed) shrinks as
//! proptest shrinks it.

use deskrinch_picture::{Formats, Limits, decode};
use deskrinch_test_pictures::hostile;
use deskrinch_untrusted::fuzz;
use proptest::prelude::*;

fn tight() -> Limits {
    Limits {
        max_encoded_bytes: 1 << 20,
        max_side: 300,
        max_decoded_bytes: 200 * 200 * 4,
        max_working_bytes: 16 << 20,
    }
}

/// Decodes `bytes` under the shared deadline (`property` names the case if
/// it runs past it) and holds the result to the limits.
fn within(property: &str, bytes: &[u8]) -> Result<(), TestCaseError> {
    fuzz::case_bytes(property, bytes, || {
        let limits = tight();
        if let Ok(decoded) = decode(bytes, Formats::all(), &limits) {
            let (w, h) = (decoded.image.width(), decoded.image.height());
            prop_assert!(w <= limits.max_side && h <= limits.max_side, "{w} × {h}");
            prop_assert!(decoded.image.rgba().len() <= limits.max_decoded_bytes);
        }
        Ok(())
    })
}

proptest! {
    #![proptest_config(fuzz::config())]

    #[test]
    fn arbitrary_bytes_never_panic_or_pass_the_limits(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        within("arbitrary_bytes_never_panic_or_pass_the_limits", &bytes)?;
    }

    #[test]
    fn a_signature_then_arbitrary_bytes_never_panic_or_pass_the_limits(
        which in 0..4_usize,
        rest in prop::collection::vec(any::<u8>(), 0..4096),
    ) {
        let signature: &[u8] = [
            b"\x89PNG\r\n\x1a\n".as_slice(),
            &[0xff, 0xd8, 0xff],
            b"GIF89a",
            b"RIFF\0\0\0\0WEBPVP8L",
        ][which];
        within(
            "a_signature_then_arbitrary_bytes_never_panic_or_pass_the_limits",
            &[signature, &rest].concat(),
        )?;
    }

    /// The corpus is made when deskrinch-test-pictures is built, so no case
    /// pays for encoding it. The first case did, in each forked child of the
    /// configuration's old timeout, when it was made at run time: 3.6 s
    /// idle, 13 to 24 s at loads of 58 to 105, which failed the limit of
    /// 10 s it had then. Measured on 2026-11-23 in a
    /// debug build: the slowest case's decode took 8.9 ms of CPU (19.5 ms of
    /// wall at a load of 59), the whole slowest case 22 ms of wall at 78.
    #[test]
    fn damaged_pictures_never_panic_or_pass_the_limits(
        which in any::<prop::sample::Index>(),
        flips in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
        end in any::<prop::sample::Index>(),
    ) {
        let corpus = hostile::ordinary();
        let mut bytes = corpus[which.index(corpus.len())].bytes.clone();
        for (at, value) in flips {
            let at = at.index(bytes.len());
            bytes[at] = value;
        }
        bytes.truncate(end.index(bytes.len()) + 1);
        within("damaged_pictures_never_panic_or_pass_the_limits", &bytes)?;
    }
}

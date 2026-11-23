//! Arbitrary and damaged bytes never panic, and never give a picture over
//! the limits. (That they never allocate past the working bound is
//! `tests/working_bound.rs`'s.)

use deskrinch_picture::{Formats, Limits, decode};
use deskrinch_test_pictures::hostile;
use proptest::prelude::*;

fn tight() -> Limits {
    Limits {
        max_encoded_bytes: 1 << 20,
        max_side: 300,
        max_decoded_bytes: 200 * 200 * 4,
        max_working_bytes: 16 << 20,
    }
}

fn within(bytes: &[u8]) -> Result<(), TestCaseError> {
    let limits = tight();
    if let Ok(decoded) = decode(bytes, Formats::all(), &limits) {
        let (w, h) = (decoded.image.width(), decoded.image.height());
        prop_assert!(w <= limits.max_side && h <= limits.max_side, "{w} × {h}");
        prop_assert!(decoded.image.rgba().len() <= limits.max_decoded_bytes);
    }
    Ok(())
}

proptest! {
    #![proptest_config(deskrinch_untrusted::fuzz::config())]

    #[test]
    fn arbitrary_bytes_never_panic_or_pass_the_limits(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        within(&bytes)?;
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
        within(&[signature, &rest].concat())?;
    }

    /// The shared configuration's 10 s limit times each case, in a forked
    /// child: a copy of a picture of the corpus, damaged, and its decode.
    /// The corpus is made when deskrinch-test-pictures is built, so no case
    /// pays for encoding it (the first case did, in each child: 3.6 s idle,
    /// over 20 s loaded, which load pushed past the limit). Measured at a
    /// load of 78: the corpus's copy at most 7 ms, the slowest case 22 ms.
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
        within(&bytes)?;
    }
}

//! The working bound held, measured: a counting global allocator
//! (`peak_alloc`) records the most a decode has allocated at once beyond
//! what was allocated before it (the input), and that must never pass the
//! decode's `max_working_bytes`.
//!
//! - **Each picture of the corpus at its own bound:** the smallest
//!   `max_working_bytes` at which it is decoded (with its colour profile
//!   used, when it has one the caps allow) is found, and decoding at exactly
//!   that bound must stay within it. This is what holds the estimates that
//!   rest on a decoder's behaviour (zune-jpeg's rows and coefficients,
//!   image-webp's planes) to the truth: an update that allocates more fails
//!   here.
//! - **Hostile pictures and mutated ones at fixed limits:** whatever the
//!   outcome, the peak stays within the bound.
//!
//! Its own binary without the test harness, so that nothing else allocates
//! while a decode is counted.

use deskrinch_picture::{Formats, Ignored, Limits, MAX_PROFILE_BYTES, Profile, decode};
use deskrinch_test_pictures::hostile::{self, Case};
use peak_alloc::PeakAlloc;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

#[global_allocator]
static COUNTED: PeakAlloc = PeakAlloc;

/// Limits that admit every picture of the corpus but the working bound.
fn limits(working: usize) -> Limits {
    Limits {
        max_encoded_bytes: 64 << 20,
        max_side: 4096,
        max_decoded_bytes: 16 << 20,
        max_working_bytes: working,
    }
}

/// Decodes `bytes` under `limits`; the outcome, and the most allocated at
/// once beyond what was allocated before (the result is kept until after
/// the measurement).
fn measured(bytes: &[u8], limits: &Limits) -> (Result<Profile, String>, usize) {
    let before = COUNTED.current_usage();
    COUNTED.reset_peak_usage();
    let result = decode(bytes, Formats::all(), limits);
    let peak = COUNTED.peak_usage().saturating_sub(before);
    (result.map(|d| d.profile).map_err(|e| e.to_string()), peak)
}

/// Whether a decode at `working` succeeds with its profile used as well as
/// the caps allow (not dropped for the working bound).
fn accepted(bytes: &[u8], working: usize) -> bool {
    matches!(
        decode(bytes, Formats::all(), &limits(working)),
        Ok(decoded) if decoded.profile != Profile::Ignored(Ignored::OverWorkingBound)
    )
}

/// The smallest working bound at which `bytes` is accepted.
fn smallest_bound(bytes: &[u8]) -> Option<usize> {
    let mut high = 256 << 20;
    if !accepted(bytes, high) {
        return None;
    }
    let mut low = 0;
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if accepted(bytes, mid) {
            high = mid;
        } else {
            low = mid;
        }
    }
    Some(high)
}

fn each_picture_stays_within_its_own_bound() {
    // Every picture is measured before any failure is reported, so that one
    // run shows them all.
    let mut failures = Vec::new();
    for Case { name, bytes } in hostile::ordinary().iter().chain(hostile::metadata()) {
        let Some(bound) = smallest_bound(bytes) else {
            failures.push(format!("{name}: never decoded"));
            continue;
        };
        let (result, peak) = measured(bytes, &limits(bound));
        println!("{name}: peak {peak} of {bound} ({}%)", peak * 100 / bound);
        if result.is_err() {
            failures.push(format!("{name}: {result:?}"));
        }
        if peak > bound {
            failures.push(format!("{name}: {peak} bytes at its peak, bound {bound}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// What the crate's documentation says a refused picture may have
/// allocated: the header read's copies of the file's metadata, at most
/// twice the encoded size and a PNG's colour profile up to the cap.
fn refused_metadata_stays_within_what_the_documentation_says() {
    let mut failures = Vec::new();
    for Case { name, bytes } in hostile::metadata() {
        let Some(own) = smallest_bound(bytes) else {
            continue;
        };
        let bound = own / 2;
        let (result, peak) = measured(bytes, &limits(bound));
        let allowed = bound.max(2 * bytes.len() + MAX_PROFILE_BYTES);
        println!("{name}, refused at {bound}: {result:?}, peak {peak} of {allowed}");
        if result.is_ok() {
            failures.push(format!("{name}: decoded at half its own bound"));
        }
        if peak > allowed {
            failures.push(format!(
                "{name}: {peak} bytes before its refusal, {allowed} allowed"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

fn hostile_pictures_stay_within_the_bound() {
    let bound = 32 << 20;
    for Case { name, bytes } in hostile::hostile() {
        let (result, peak) = measured(&bytes, &limits(bound));
        assert!(
            peak <= bound,
            "{name}: {peak} bytes at its peak ({result:?})"
        );
        println!("{name}: {result:?}, peak {peak}");
    }
}

fn mutated_pictures_stay_within_the_bound() {
    let corpus = hostile::ordinary();
    let bound = 16 << 20;
    let tight = Limits {
        max_encoded_bytes: 1 << 20,
        max_side: 512,
        max_decoded_bytes: 1 << 20,
        max_working_bytes: bound,
    };
    let strategy = (
        0..corpus.len(),
        prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
        any::<prop::sample::Index>(),
    );
    // The shared configuration's cases, run in this process: its forked
    // children (for the timeout) re-run a named test of the harness, which
    // this binary does not have.
    let config = proptest::test_runner::Config {
        timeout: 0,
        fork: false,
        failure_persistence: None,
        ..deskrinch_untrusted::fuzz::config()
    };
    let mut runner = TestRunner::new(config);
    runner
        .run(&strategy, |(which, flips, end)| {
            let mut bytes = corpus[which].bytes.clone();
            for (at, value) in flips {
                let at = at.index(bytes.len());
                bytes[at] = value;
            }
            bytes.truncate(end.index(bytes.len()) + 1);
            let (result, peak) = measured(&bytes, &tight);
            prop_assert!(peak <= bound, "{peak} bytes at its peak ({result:?})");
            Ok(())
        })
        .unwrap_or_else(|failure| panic!("{failure}"));
}

fn main() {
    each_picture_stays_within_its_own_bound();
    refused_metadata_stays_within_what_the_documentation_says();
    hostile_pictures_stay_within_the_bound();
    mutated_pictures_stay_within_the_bound();
    println!("working bound: all held");
}

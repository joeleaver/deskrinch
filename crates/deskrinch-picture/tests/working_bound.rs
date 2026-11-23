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

use deskrinch_picture::{
    Fit, FitError, Formats, Ignored, Limits, MAX_PROFILE_BYTES, Profile, decode, fit,
};
use deskrinch_test_pictures::hostile::{self, Case};
use deskrinch_untrusted::{Image, fuzz};
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
    // The shared configuration, with no file of failures kept: proptest
    // keeps them beside the source of a `proptest!` test, which a runner
    // made by hand has none of.
    let config = proptest::test_runner::Config {
        failure_persistence: None,
        ..fuzz::config()
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
            // The watchdog is armed before the measurement begins, so its
            // own allocations are not in the peak.
            fuzz::case_bytes("mutated_pictures_stay_within_the_bound", &bytes, || {
                let (result, peak) = measured(&bytes, &tight);
                prop_assert!(peak <= bound, "{peak} bytes at its peak ({result:?})");
                Ok(())
            })
        })
        .unwrap_or_else(|failure| panic!("{failure}"));
}

/// The bound a fit says it needs, held to being exact and to the truth:
/// fitted under a working bound of zero, the refusal says what it needs;
/// at exactly that it fits, one byte under it is refused with the same
/// figure, and the measured peak at it stays within it. Gives the peak,
/// the bound and the fit's time, or what failed.
fn exact_fit_bound(
    fitting: impl Fn(usize) -> Result<Image, FitError>,
) -> Result<(usize, usize, std::time::Duration), String> {
    let needed = match fitting(0) {
        Err(FitError::WorkingTooLarge {
            needed: Some(needed),
            ..
        }) => needed,
        other => return Err(format!("under a bound of zero: {other:?}")),
    };
    match fitting(needed - 1) {
        Err(FitError::WorkingTooLarge {
            needed: Some(again),
            ..
        }) if again == needed => {}
        other => return Err(format!("a byte under {needed}: {other:?}")),
    }
    let before = COUNTED.current_usage();
    COUNTED.reset_peak_usage();
    let started = std::time::Instant::now();
    let result = fitting(needed);
    let took = started.elapsed();
    let peak = COUNTED.peak_usage().saturating_sub(before);
    match result {
        Ok(_) if peak <= needed => Ok((peak, needed, took)),
        Ok(_) => Err(format!("{peak} bytes at its peak, bound {needed}")),
        Err(error) => Err(format!("at exactly {needed}: {error:?}")),
    }
}

/// Each fit, smaller and larger, at the bound its refusal names.
fn each_fit_stays_within_its_own_bound() {
    let source = Image::new(1200, 800, hostile::noise(1200 * 800 * 4))
        .unwrap_or_else(|| panic!("a picture"));
    let mut failures = Vec::new();
    for how in [
        Fit::Cover,
        Fit::Contain,
        Fit::Stretch,
        Fit::Centre,
        Fit::Tile,
    ] {
        for (w, h) in [(1920, 1080), (640, 360), (2400, 1600)] {
            match exact_fit_bound(|working| fit(&source, w, h, how, &limits(working))) {
                Ok((peak, bound, _)) => {
                    println!(
                        "fit {how:?} to {w}x{h}: peak {peak} of {bound} ({}%)",
                        peak * 100 / bound
                    );
                }
                Err(failure) => failures.push(format!("{how:?} to {w}x{h}: {failure}")),
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A 5120 × 2880 picture (a 5K wallpaper) fitted to 1920 × 1080, opaque
/// and with alpha: its working memory must follow the target, not the
/// source. At the bound its refusal names; the time is printed beside it.
fn a_5k_picture_fits_in_what_follows_the_target() {
    let (sw, sh) = (5120_u32, 2880_u32);
    let mut failures = Vec::new();
    for opaque in [true, false] {
        let mut rgba = hostile::noise(sw as usize * sh as usize * 4);
        if opaque {
            for alpha in rgba.iter_mut().skip(3).step_by(4) {
                *alpha = u8::MAX;
            }
        }
        let source = Image::new(sw, sh, rgba).unwrap_or_else(|| panic!("a picture"));
        let kind = if opaque { "opaque" } else { "with alpha" };
        for how in [Fit::Cover, Fit::Stretch] {
            match exact_fit_bound(|working| fit(&source, 1920, 1080, how, &limits(working))) {
                Ok((peak, bound, took)) => println!(
                    "5K {how:?}, {kind}: peak {peak} of {bound} ({}%), {took:?}",
                    peak * 100 / bound
                ),
                Err(failure) => failures.push(format!("5K {how:?}, {kind}: {failure}")),
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

fn main() {
    a_5k_picture_fits_in_what_follows_the_target();
    each_fit_stays_within_its_own_bound();
    each_picture_stays_within_its_own_bound();
    refused_metadata_stays_within_what_the_documentation_says();
    hostile_pictures_stay_within_the_bound();
    mutated_pictures_stay_within_the_bound();
    println!("working bound: all held");
}

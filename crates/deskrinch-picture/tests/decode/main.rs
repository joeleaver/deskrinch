//! deskrinch-picture's decoding, format by format, from pictures made by
//! `deskrinch-test-pictures`: what each gives, what each limit refuses, the
//! colour profiles, the file read, and arbitrary bytes. The working bound
//! is measured in `tests/working_bound.rs`.

// `cfg(test)` marks it all as test code, where clippy allows `unwrap`.
#[cfg(test)]
mod colour;
#[cfg(test)]
mod file;
#[cfg(test)]
mod gif;
#[cfg(test)]
mod jpeg;
#[cfg(test)]
mod limits;
#[cfg(test)]
mod png;
#[cfg(test)]
mod props;
#[cfg(test)]
mod support;
#[cfg(test)]
mod webp;

#[cfg(test)]
use support::{decoded, near, pixel, quadrants, roomy};

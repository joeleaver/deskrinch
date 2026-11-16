//! The shared rules for what an untrusted peer on the bus may hand over:
//! strings cleaned of control characters and bounded, markup parsed into
//! styled text that carries no markup, pixel data validated into plain
//! RGBA, themed icon names that cannot name a path, and a file a peer names
//! read under a bound ([`file::read_bounded`]). Encoded pictures (PNG and
//! the rest) are decoded by `deskrinch-picture`, into this crate's
//! [`Image`].
//!
//! The notification service and the status-notifier host both take input
//! from applications they do not trust; the rules each applies to a string
//! or an image are these, in one place.
//!
//! # Example
//!
//! ```
//! use deskrinch_untrusted::{IconName, Image, text};
//!
//! assert_eq!(text::clean("a\u{7}b\r\nc", 64, text::Lines::Join), "ab c");
//! assert!(IconName::parse("../../etc/passwd").is_none());
//! assert!(Image::new(1, 1, vec![0, 0, 0, 255]).is_some());
//! ```

pub mod file;
#[cfg(any(test, feature = "proptest"))]
pub mod fuzz;
mod image;
pub mod markup;
pub mod text;

pub use image::{IconName, Image};
